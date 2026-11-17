//! File MIME targets on the GTK UI thread, including sandbox-aware file transfers.
use gtk::{gio, glib};
use gio::prelude::*;
use glib::variant::ToVariant;
use glib::translate::ToGlibPtr;
use std::collections::HashMap;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

const BUS: &str = "org.freedesktop.portal.Documents";
const OBJECT: &str = "/org/freedesktop/portal/documents";
const INTERFACE: &str = "org.freedesktop.portal.FileTransfer";

struct Transfer { connection: gio::DBusConnection, key: String }

impl Transfer {
    fn new(path: &Path) -> Result<Self, String> {
        let connection = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>).map_err(|e| e.to_string())?;
        let options: HashMap<String, glib::Variant> = [("autostop".into(), false.to_variant())].into();
        let answer = connection.call_sync(Some(BUS), OBJECT, INTERFACE, "StartTransfer",
            Some(&(options,).to_variant()), None, gio::DBusCallFlags::NONE, 2000, None::<&gio::Cancellable>)
            .map_err(|e| e.to_string())?;
        let key: String = answer.child_value(0).get().ok_or("Invalid file transfer response")?;
        let transfer = Self { connection, key };
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let descriptors = gio::UnixFDList::new();
        let index = descriptors.append(file).map_err(|e| e.to_string())?;
        let options = HashMap::<String, glib::Variant>::new();
        let message = gio::DBusMessage::new_method_call(Some(BUS), OBJECT, Some(INTERFACE), "AddFiles");
        message.set_body(&(transfer.key.clone(), vec![glib::variant::Handle(index)], options).to_variant());
        message.set_unix_fd_list(Some(&descriptors));
        message.set_num_unix_fds(1);
        let (reply, _) = transfer.connection.send_message_with_reply_sync(&message, gio::DBusSendMessageFlags::NONE,
            2000, None::<&gio::Cancellable>).map_err(|e| e.to_string())?;
        // gio 0.18's generated to_gerror() has an inverted debug assertion for
        // successful method returns. Inspect the D-Bus error message directly.
        if reply.message_type() == gio::DBusMessageType::Error {
            let detail = reply.body().and_then(|body| body.child_value(0).get::<String>());
            return Err(detail.unwrap_or_else(|| reply.error_name().map(|name| name.to_string())
                .unwrap_or_else(|| "The file-transfer portal rejected the clip".into())));
        }
        Ok(transfer)
    }
}

impl Drop for Transfer {
    fn drop(&mut self) {
        let _ = self.connection.call_sync(Some(BUS), OBJECT, INTERFACE, "StopTransfer", Some(&(self.key.as_str(),).to_variant()),
            None, gio::DBusCallFlags::NONE, 500, None::<&gio::Cancellable>);
    }
}

pub struct Prepared { uri: String, transfer: Option<Transfer> }

impl Prepared {
    /// Run blocking portal requests on the export worker before entering the UI thread.
    pub fn new(path: &Path) -> Result<Self, String> {
        let path = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
        let mut uri = String::from("file://");
        for &byte in path.as_os_str().as_bytes() {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.' | b'~') { uri.push(byte as char); }
            else { uri.push_str(&format!("%{byte:02X}")); }
        }
        let transfer = match Transfer::new(&path) {
            Ok(transfer) => Some(transfer),
            Err(error) if std::env::var_os("FLATPAK_ID").is_some() => return Err(format!("The desktop file-transfer portal could not share the clip: {error}")),
            Err(_) => None, // Native sessions can exchange ordinary file URIs.
        };
        Ok(Self { uri, transfer })
    }

    /// Must run on Tauri's GTK main thread so Wayland uses the user's input serial.
    pub fn copy(self) -> Result<(), String> {
        let clipboard = gtk::Clipboard::get(&gtk::gdk::SELECTION_CLIPBOARD);
        let mut targets = vec![gtk::TargetEntry::new("text/uri-list", gtk::TargetFlags::empty(), 0),
            gtk::TargetEntry::new("x-special/gnome-copied-files", gtk::TargetFlags::empty(), 1)];
        if self.transfer.is_some() {
            targets.push(gtk::TargetEntry::new("application/vnd.portal.filetransfer", gtk::TargetFlags::empty(), 2));
        }
        if !clipboard.set_with_data(&targets, move |_, selection, target| {
            match target {
                0 => { selection.set_uris(&[&self.uri]); },
                1 => selection.set(&selection.target(), 8, format!("copy\n{}\n", self.uri).as_bytes()),
                2 => if let Some(transfer) = &self.transfer { selection.set(&selection.target(), 8, transfer.key.as_bytes()); },
                _ => {},
            }
        }) { return Err("The desktop refused clipboard ownership".into()); }
        unsafe { gtk::ffi::gtk_clipboard_set_can_store(clipboard.to_glib_none().0, std::ptr::null_mut(), 0); }
        clipboard.store();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires an X11/Wayland desktop; CI runs this under Xvfb"]
    fn linux_clipboard_exchanges_file_mime_types() {
        gtk::init().expect("GTK desktop");
        let root = std::env::temp_dir().join(format!("encore-clipboard-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("a clip ü #.mp4");
        std::fs::write(&path, b"clipboard fixture").unwrap();
        let prepared = Prepared::new(&path).unwrap();
        let uri = prepared.uri.clone();
        assert!(uri.ends_with("a%20clip%20%C3%BC%20%23.mp4"));
        prepared.copy().unwrap();
        let clipboard = gtk::Clipboard::get(&gtk::gdk::SELECTION_CLIPBOARD);
        let data = clipboard.wait_for_contents(&gtk::gdk::Atom::intern("text/uri-list")).unwrap();
        assert_eq!(data.uris().iter().map(|value| value.as_str()).collect::<Vec<_>>(), vec![uri.as_str()]);
        let data = clipboard.wait_for_contents(&gtk::gdk::Atom::intern("x-special/gnome-copied-files")).unwrap();
        assert_eq!(data.data(), format!("copy\n{uri}\n").as_bytes());
        clipboard.clear();
        std::fs::remove_dir_all(root).unwrap();
    }
}
