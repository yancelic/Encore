import { useMemo, useState } from 'react';
import { ArrowDown, ArrowUp, Layers, Trash2, X } from 'lucide-react';
import { formatDuration, mediaUrl } from '../api';
import type { ClipRecord } from '../types';
import { Modal } from './Modal';

interface MontageModalProps {
  initialClips: ClipRecord[];
  busy: boolean;
  onClose: () => void;
  onBuild: (ids: string[], title: string) => void;
}

export function MontageModal({
  initialClips,
  busy,
  onClose,
  onBuild,
}: MontageModalProps) {
  const [items, setItems] = useState<ClipRecord[]>(initialClips);
  const [title, setTitle] = useState(
    `${initialClips[0]?.game ?? 'Encore'} Highlight Montage`,
  );

  const totalMs = useMemo(
    () => items.reduce((acc, c) => acc + c.duration_ms, 0),
    [items],
  );

  const move = (idx: number, delta: -1 | 1) => {
    const nextIdx = idx + delta;
    if (nextIdx < 0 || nextIdx >= items.length) return;
    setItems((prev) => {
      const copy = [...prev];
      const [item] = copy.splice(idx, 1);
      copy.splice(nextIdx, 0, item);
      return copy;
    });
  };

  const remove = (idx: number) => {
    setItems((prev) => prev.filter((_, i) => i !== idx));
  };

  return (
    <Modal titleId="montage-title" busy={busy} onClose={onClose}>
      <div className="modal-header">
        <div className="modal-title-group">
          <Layers size={20} color="var(--primary)" />
          <div>
            <h2 id="montage-title">One great replay.</h2>
            <p className="modal-subtitle">Arrange your clips into a montage.</p>
          </div>
        </div>
        <div className="modal-title-group">
          <button
            className="btn-icon"
            onClick={onClose}
            disabled={busy}
            aria-label="Close montage"
          >
            <X size={15} />
          </button>
        </div>
      </div>

      <div className="modal-body">
        <fieldset className="modal-fields" disabled={busy}>
          <div className="field-group">
            <label htmlFor="montage-name">Montage title</label>
            <input
              type="text"
              id="montage-name"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              placeholder="Enter montage title..."
            />
          </div>

          <div
            style={{
              display: 'flex',
              justifyContent: 'space-between',
              alignItems: 'center',
            }}
          >
            <span className="inspector-label">Clip Order ({items.length})</span>
            <span
              className="mono"
              style={{ color: 'var(--ink-engraved)', fontSize: 11 }}
            >
              Total Duration: {formatDuration(totalMs)}
            </span>
          </div>

          <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
            {items.map((c, idx) => (
              <div key={c.id} className="montage-item-row">
                <span className="montage-order-badge picked mono">
                  {idx + 1}
                </span>
                {c.thumb_path && (
                  <img src={mediaUrl(c.thumb_path)} alt={c.title} />
                )}
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div className="clip-title">{c.title}</div>
                  <div
                    className="mono"
                    style={{ fontSize: 11, color: 'var(--ink-engraved)' }}
                  >
                    {c.game} · {formatDuration(c.duration_ms)}
                  </div>
                </div>
                <button
                  className="btn-icon"
                  disabled={idx === 0 || busy}
                  onClick={() => move(idx, -1)}
                  title="Move up"
                >
                  <ArrowUp size={13} />
                </button>
                <button
                  className="btn-icon"
                  disabled={idx === items.length - 1 || busy}
                  onClick={() => move(idx, 1)}
                  title="Move down"
                >
                  <ArrowDown size={13} />
                </button>
                <button
                  className="btn-icon"
                  disabled={items.length <= 2 || busy}
                  onClick={() => remove(idx)}
                  title="Remove from montage"
                >
                  <Trash2 size={13} />
                </button>
              </div>
            ))}
          </div>
        </fieldset>
      </div>

      <div className="modal-footer">
        <button className="btn-ghost" onClick={onClose} disabled={busy}>
          Cancel
        </button>
        <button
          className="btn-primary"
          disabled={items.length < 2 || !title.trim() || busy}
          onClick={() =>
            onBuild(
              items.map((x) => x.id),
              title,
            )
          }
        >
          {busy ? 'Creating montage…' : 'Create montage'}
        </button>
      </div>
    </Modal>
  );
}
