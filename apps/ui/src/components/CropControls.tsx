import { useLayoutEffect, useRef, useState } from 'react';
export interface CropRect {
  x: number;
  y: number;
  width: number;
  height: number;
}
export const DEFAULT_CROP: CropRect = {
  x: 0.1,
  y: 0.1,
  width: 0.8,
  height: 0.8,
};
export function CropOverlay({
  crop,
  onChange,
  aspect,
}: {
  crop: CropRect;
  onChange: (crop: CropRect) => void;
  aspect: number;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ width: 0, height: 0 });
  useLayoutEffect(() => {
    const measure = () => {
      const rect = ref.current!.getBoundingClientRect();
      const width = Math.min(rect.width, rect.height * aspect);
      setSize({ width, height: width / aspect });
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(ref.current!);
    return () => observer.disconnect();
  }, [aspect]);
  return (
    <div ref={ref} className="crop-overlay">
      <div className="crop-image-bounds" style={size}>
        <div
          className="custom-crop-box"
          style={{
            left: `${crop.x * 100}%`,
            top: `${crop.y * 100}%`,
            width: `${crop.width * 100}%`,
            height: `${crop.height * 100}%`,
          }}
          onPointerDown={(event) => {
            event.preventDefault();
            event.currentTarget.setPointerCapture(event.pointerId);
            const start = { x: event.clientX, y: event.clientY, crop };
            const resize = (event.target as HTMLElement).closest(
              '.crop-resize',
            );
            const move = (next: PointerEvent) => {
              const dx = (next.clientX - start.x) / size.width;
              const dy = (next.clientY - start.y) / size.height;
              onChange(
                resize
                  ? {
                      ...crop,
                      width: Math.max(
                        0.05,
                        Math.min(1 - crop.x, crop.width + dx),
                      ),
                      height: Math.max(
                        0.05,
                        Math.min(1 - crop.y, crop.height + dy),
                      ),
                    }
                  : {
                      ...crop,
                      x: Math.max(0, Math.min(1 - crop.width, crop.x + dx)),
                      y: Math.max(0, Math.min(1 - crop.height, crop.y + dy)),
                    },
              );
            };
            const element = event.currentTarget;
            const stop = () => {
              element.removeEventListener('pointermove', move);
              element.removeEventListener('pointerup', stop);
              element.removeEventListener('pointercancel', stop);
            };
            element.addEventListener('pointermove', move);
            element.addEventListener('pointerup', stop);
            element.addEventListener('pointercancel', stop);
          }}
        >
          <span className="crop-box-label">Custom crop</span>
          <span className="crop-resize" title="Drag to resize crop" />
        </div>
      </div>
    </div>
  );
}
export function CropControls({
  crop,
  onChange,
  width,
  height,
}: {
  crop: CropRect;
  onChange: (crop: CropRect) => void;
  width: number;
  height: number;
}) {
  return (
    <div className="custom-crop-controls">
      <p>Drag the frame to move it. Drag its bottom-right corner to resize.</p>
      <div className="crop-fields">
        {(['x', 'y', 'width', 'height'] as const).map((key) => (
          <label key={key}>
            {{ x: 'Left', y: 'Top', width: 'Width', height: 'Height' }[key]} (%)
            <input
              type="number"
              aria-label={`Crop ${key} percent`}
              min={key === 'width' || key === 'height' ? 5 : 0}
              max={100}
              step={1}
              value={Math.round(crop[key] * 100)}
              onChange={(event) => {
                const value = event.target.valueAsNumber / 100;
                if (!Number.isFinite(value)) return;
                const next = {
                  ...crop,
                  [key]: Math.max(
                    key === 'width' || key === 'height' ? 0.05 : 0,
                    Math.min(1, value),
                  ),
                };
                next.x = Math.min(next.x, 1 - next.width);
                next.y = Math.min(next.y, 1 - next.height);
                onChange(next);
              }}
            />
          </label>
        ))}
      </div>
      <span>
        Output: {Math.max(2, Math.floor((width * crop.width) / 2) * 2)} ×{' '}
        {Math.max(2, Math.floor((height * crop.height) / 2) * 2)}
      </span>
      <button className="btn-secondary" onClick={() => onChange(DEFAULT_CROP)}>
        Reset crop
      </button>
    </div>
  );
}
