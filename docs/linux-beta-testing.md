# Encore — Ubuntu beta test notu

Hedef: **Ubuntu 24.04+ x64 (amd64)**. Bu deneysel bir geliştirme sürümüdür.
İndirdiğin beta etiketini ve `.deb` dosya adını hata raporuna ekle.

## Kurulum

`.deb` dosyasının bulunduğu klasörde terminal aç:

```sh
sudo apt install ./Encore_*_amd64.deb
```

Klasörde yalnızca kuracağın Encore paketi bulunsun. Gerekli bağımlılıkları APT
kurar; Rust, Node.js veya OBS Studio arayüzünü kurman gerekmez. Menüden Encore'u
aç veya terminalde `encore-ui` çalıştır.

## İlk test

1. Settings'ten 720p, 30 FPS ve 15 saniye replay seçip kaydet.
2. Wayland'de sistem paylaşım penceresinden oyun/test penceresini seç. Gerekirse
   **Choose recording window…** kullan. X11'de çalışan uygulamayı Games listesine
   ekleyip ayarları kaydet.
3. 20 saniye hareketli içerik göster; **Save replay** ile klip kaydet. Görüntü
   hareketli mi, doğru pencere mi, sesle eşzamanlı mı kontrol et.
4. Birkaç saniye arayla üç klip daha kaydet. Kaydın devam ettiğini kontrol et.
5. Alt+S'yi, ardından Enter/ok/numpad gibi bir özel tuşla başka bir kısayolu dene.
   Wayland'de masaüstü izin isteyebilir veya seçtiğin kombinasyonu değiştirebilir.
   Global Shortcuts portalı yoksa ya da izin verilmezse kütüphanedeki
   **Save replay** düğmesini kullan ve durumu not et.
6. Oyun, sohbet ve mikrofon kaynaklarını ayrı ayrı seçip editörün mute/volume
   kontrolleriyle gerçekten ayrıştıklarını dinle. Wayland uygulama sesi modunda
   oyun ses uygulamasını ayrıca seç; paylaşım penceresi bu bilgiyi sağlamaz.
7. Başka pencereye geç, seçilen pencereyi küçült/geri getir; sonucu not et.
   Kütüphane penceresini kapatıp tekrar açınca kaydın devamını da dene.
8. Klibi uygulamada oynat, kırp, dışa aktar ve dosya yöneticisine kopyala/yapıştır.
9. İlk testler geçerse 1080p60 / 60 saniye replay ile 70 saniye bekleyip kaydet.
   Takılma, ses kayması veya belirgin performans düşüşü varsa not et.

Test bitince uygulamanın **Quit Encore** komutuyla arka plan kaydını durdur.

## Hata olursa

- Beta etiketi, Ubuntu sürümü, masaüstü adı, Wayland/X11, ekran kartı ve sürücüsü.
- Kayıt ayarları, oyun/uygulama adı ve hatayı tekrarlayan adımlar.
- Beklenen sonuç, gerçekleşen sonuç ve varsa hata mesajının ekran görüntüsü.
- Kayıt sorunu için recorder.log dosyasının son 200 satırı:

```sh
tail -n 200 "${XDG_CONFIG_HOME:-$HOME/.config}/Encore/recorder.log"
```

Sistem bilgisi için:

```sh
cat /etc/os-release
printf 'Oturum: %s\nMasaüstü: %s\n' "$XDG_SESSION_TYPE" "$XDG_CURRENT_DESKTOP"
lspci -nnk | grep -A3 -E 'VGA|3D|Display'
```

Uygulama hiç açılmazsa terminal çıktısını dosyaya al:

```sh
encore-ui > encore-ui-terminal.log 2>&1
```

Loglarda yerel dosya yolları ve uygulama adları bulunabilir; göndermeden önce
gözden geçir. Paketle aynı klasöre checksum ve test notunu da indirirsen dosyaları
şu komutla doğrulayabilirsin:

```sh
sha256sum -c SHA256SUMS.txt
```
