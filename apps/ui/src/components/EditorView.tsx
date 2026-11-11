import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  ArrowLeft,
  Check,
  ChevronLeft,
  ChevronRight,
  ClipboardCopy,
  Download,
  FolderOpen,
  Gamepad2,
  Mic,
  Pause,
  Play,
  Scissors,
  SkipBack,
  SkipForward,
  Sparkles,
  Trash2,
  Users,
  Volume1,
  Volume2,
  VolumeX,
  X,
} from 'lucide-react';
import {
  formatBytes,
  formatDateTime,
  formatTimecode,
  getClipAudioTracks,
  getClipWaveforms,
  mediaUrl,
} from '../api';
import type { ClipRecord } from '../types';
import type { ResolvedTheme } from '../theme';
import { CropControls, CropOverlay, DEFAULT_CROP, type CropRect } from './CropControls';

interface EditorViewProps {
  theme: ResolvedTheme;
  clip: ClipRecord;
  busyAction: string | null;
  onBack: () => void;
  onSaveDetails: (id: string, title: string, tag: string, description: string) => Promise<void>;
  onTrimClip: (
    id: string,
    startSecs: number,
    endSecs: number,
    mutedTracks: boolean[],
    trackVolumes: number[],
  ) => Promise<void>;
  onExportVertical: (
    id: string,
    blurBg: boolean,
    startSecs: number | null,
    endSecs: number | null,
    mutedTracks: boolean[],
    trackVolumes: number[],
    crop?: CropRect,
  ) => Promise<void>;
  onCopyClip: (path: string) => Promise<void>;
  onRevealClip: (path: string) => Promise<void>;
  onDeleteClip: (clip: ClipRecord) => Promise<void>;
}

const TRACK_META = [
  { label: 'Game audio', color: 'var(--game-track)', icon: Gamepad2 },
  { label: 'Chat audio', color: 'var(--chat-track)', icon: Users },
  { label: 'Microphone', color: 'var(--mic-track)', icon: Mic },
] as const;

interface WaveformCanvasProps {
  theme: ResolvedTheme;
  peaks: number[];
  color: string;
  muted: boolean;
  volume: number;
  trimStartRatio: number;
  trimEndRatio: number;
}

function WaveformCanvas({
  theme,
  peaks,
  color,
  muted,
  volume,
  trimStartRatio,
  trimEndRatio,
}: WaveformCanvasProps) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const [size, setSize] = useState<{ w: number; h: number }>({ w: 600, h: 36 });

  useEffect(() => {
    const el = canvasRef.current;
    if (!el) return;
    const ro = new ResizeObserver((entries) => {
      for (const e of entries) {
        const w = Math.max(60, Math.floor(e.contentRect.width));
        const h = Math.max(20, Math.floor(e.contentRect.height));
        setSize({ w, h });
      }
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.floor(size.w * dpr);
    canvas.height = Math.floor(size.h * dpr);
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, size.w, size.h);

    const cy = size.h / 2;
    const palette = getComputedStyle(canvas);
    const trackColor = palette.getPropertyValue(color.slice(4, -1)).trim();
    ctx.strokeStyle = palette.getPropertyValue('--waveform-grid').trim();
    ctx.lineWidth = 1;
    for (let gx = 40; gx < size.w; gx += 40) {
      ctx.beginPath();
      ctx.moveTo(gx, 0);
      ctx.lineTo(gx, size.h);
      ctx.stroke();
    }
    ctx.strokeStyle = palette.getPropertyValue('--waveform-line').trim();
    ctx.beginPath();
    ctx.moveTo(0, cy);
    ctx.lineTo(size.w, cy);
    ctx.stroke();

    const data = peaks.length ? peaks : Array.from({ length: 120 }, () => 0.04);
    const step = size.w / data.length;
    const barW = Math.max(1.5, step * 0.68);
    const maxAmp = (size.h - 6) * 0.48;

    const effectiveMuted = muted || volume <= 0.001;
    const fill = effectiveMuted ? palette.getPropertyValue('--ink-dim').trim() : trackColor;

    for (let i = 0; i < data.length; i++) {
      const ratio = (i + 0.5) / data.length;
      const inTrim = ratio >= trimStartRatio && ratio <= trimEndRatio;
      const scaled = effectiveMuted ? data[i] : data[i] * volume;
      const amp = Math.max(0.04, Math.min(1, scaled)) * maxAmp;
      const x = i * step + (step - barW) * 0.5;

      ctx.globalAlpha = effectiveMuted ? 0.55 : inTrim ? 0.92 : 0.22;
      ctx.fillStyle = fill;
      ctx.fillRect(x, cy - amp, barW, amp * 2);
    }
    ctx.globalAlpha = 1;
  }, [peaks, color, muted, volume, trimStartRatio, trimEndRatio, size, theme]);

  return <canvas ref={canvasRef} className="waveform-canvas" />;
}

export function EditorView({
  theme,
  clip,
  busyAction,
  onBack,
  onSaveDetails,
  onTrimClip,
  onExportVertical,
  onCopyClip,
  onRevealClip,
  onDeleteClip,
}: EditorViewProps) {
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const audioRefs = useRef<(HTMLAudioElement | null)[]>([null, null, null]);
  const videoLaneRef = useRef<HTMLDivElement | null>(null);

  const initialDur = Math.max(0.2, clip.duration_ms / 1000);
  const [duration, setDuration] = useState<number>(initialDur);
  const [currentTime, setCurrentTime] = useState<number>(0);
  const [playing, setPlaying] = useState<boolean>(false);
  const [volume, setVolume] = useState<number>(0.85);
  const [trimStart, setTrimStart] = useState<number>(0);
  const [trimEnd, setTrimEnd] = useState<number>(initialDur);

  const [mutedTracks, setMutedTracks] = useState<boolean[]>([false, false, false]);
  const [trackVolumes, setTrackVolumes] = useState<number[]>([1, 1, 1]);
  const [waveforms, setWaveforms] = useState<number[][]>([]);
  const [trackAudioUrls, setTrackAudioUrls] = useState<string[]>([]);

  const [canvasMode, setCanvasMode] = useState<'original' | '9:16' | 'custom'>('original');
  const [crop, setCrop] = useState<CropRect>(DEFAULT_CROP);
  const [blurBg, setBlurBg] = useState<boolean>(true);
  const [title, setTitle] = useState<string>(clip.title);
  const [tag, setTag] = useState<string>(clip.tag);
  const [description, setDescription] = useState<string>(clip.description);
  const [detailsSaved, setDetailsSaved] = useState<boolean>(false);
  const [copiedFeedback, setCopiedFeedback] = useState<boolean>(false);
  const [confirmDelete, setConfirmDelete] = useState<boolean>(false);

  const dragRef = useRef<'start' | 'end' | 'scrub' | null>(null);

  useEffect(() => {
    const dur = Math.max(0.2, clip.duration_ms / 1000);
    setDuration(dur);
    setCurrentTime(0);
    setPlaying(false);
    setTrimStart(0);
    setTrimEnd(dur);
    setMutedTracks([false, false, false]);
    setTrackVolumes([1, 1, 1]);
    setTitle(clip.title);
    setTag(clip.tag);
    setDescription(clip.description);
    setDetailsSaved(false);
    setConfirmDelete(false);
    setCrop(DEFAULT_CROP);
    setCanvasMode('original');
    setWaveforms([]);
    setTrackAudioUrls([]);

    let cancelled = false;
    getClipWaveforms(clip.id)
      .then((wf) => {
        if (!cancelled) setWaveforms(wf);
      })
      .catch(() => {});
    getClipAudioTracks(clip.id)
      .then((paths) => {
        if (!cancelled) setTrackAudioUrls(paths.map(mediaUrl));
      })
      .catch(() => {});

    return () => {
      cancelled = true;
    };
  }, [clip.id]);

  const multiTrackAudioActive = trackAudioUrls.length >= 2;

  const applyAudioMix = useCallback(() => {
    const v = videoRef.current;
    if (!v) return;
    if (!multiTrackAudioActive) {
      const t0Vol = Math.max(0, Math.min(1, trackVolumes[0] ?? 1));
      v.muted = Boolean(mutedTracks[0]) || t0Vol <= 0.001;
      v.volume = Math.max(0, Math.min(1, volume * t0Vol));
      return;
    }
    v.muted = true;
    for (let i = 0; i < trackAudioUrls.length; i++) {
      const a = audioRefs.current[i];
      if (!a) continue;
      const tVol = Math.max(0, Math.min(1, trackVolumes[i] ?? 1));
      a.muted = Boolean(mutedTracks[i]) || tVol <= 0.001;
      a.volume = Math.max(0, Math.min(1, volume * tVol));
    }
  }, [multiTrackAudioActive, mutedTracks, trackVolumes, trackAudioUrls.length, volume]);

  const syncAudioElements = useCallback(
    (forceSeek = false) => {
      applyAudioMix();
      const v = videoRef.current;
      if (!v || !multiTrackAudioActive) return;
      for (let i = 0; i < trackAudioUrls.length; i++) {
        const a = audioRefs.current[i];
        if (!a) continue;
        if (forceSeek || Math.abs(a.currentTime - v.currentTime) > 0.08) {
          a.currentTime = v.currentTime;
        }
        if (!v.paused && a.paused) {
          a.play().catch(() => {});
        } else if (v.paused && !a.paused) {
          a.pause();
        }
      }
    },
    [applyAudioMix, multiTrackAudioActive, trackAudioUrls.length],
  );

  useEffect(() => {
    applyAudioMix();
  }, [applyAudioMix]);

  useEffect(() => {
    syncAudioElements(true);
  }, [multiTrackAudioActive, trackAudioUrls, syncAudioElements]);

  useEffect(() => {
    if (!playing) return;
    let raf = 0;
    const tick = () => {
      const v = videoRef.current;
      if (v) {
        setCurrentTime(v.currentTime);
        syncAudioElements(false);
      }
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [playing, syncAudioElements]);

  const seekTo = useCallback(
    (secs: number) => {
      const v = videoRef.current;
      if (!v) return;
      const clamped = Math.max(0, Math.min(duration, secs));
      v.currentTime = clamped;
      setCurrentTime(clamped);
      syncAudioElements(true);
    },
    [duration, syncAudioElements],
  );

  const togglePlay = useCallback(() => {
    const v = videoRef.current;
    if (!v) return;
    if (v.paused) {
      if (v.currentTime >= duration - 0.05) {
        seekTo(trimStart);
      }
      v.play().catch(() => {});
    } else {
      v.pause();
    }
  }, [duration, seekTo, trimStart]);

  // Editor keyboard shortcuts
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      const el = e.target as HTMLElement | null;
      if (el?.closest('dialog') || document.querySelector('dialog[open]')) return;
      const isRange = el instanceof HTMLInputElement && el.type === 'range';
      if (
        el &&
        !isRange &&
        (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.tagName === 'SELECT')
      ) {
        return;
      }
      if (isRange && (e.code === 'ArrowLeft' || e.code === 'ArrowRight')) {
        return;
      }
      if (e.code === 'Space') {
        e.preventDefault();
        togglePlay();
      } else if (e.code === 'ArrowLeft') {
        e.preventDefault();
        const step = 1 / Math.max(15, clip.fps || 60);
        seekTo(currentTime - (e.shiftKey ? 1 : step));
      } else if (e.code === 'ArrowRight') {
        e.preventDefault();
        const step = 1 / Math.max(15, clip.fps || 60);
        seekTo(currentTime + (e.shiftKey ? 1 : step));
      } else if (e.key === 'i' || e.key === 'I') {
        setTrimStart(Math.min(currentTime, trimEnd - 0.15));
      } else if (e.key === 'o' || e.key === 'O') {
        setTrimEnd(Math.max(currentTime, trimStart + 0.15));
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [togglePlay, seekTo, currentTime, clip.fps, trimStart, trimEnd]);

  // Timeline drag handlers
  useEffect(() => {
    const onMove = (e: PointerEvent) => {
      const kind = dragRef.current;
      const lane = videoLaneRef.current;
      if (!kind || !lane) return;
      const rect = lane.getBoundingClientRect();
      const ratio = Math.max(0, Math.min(1, (e.clientX - rect.left) / Math.max(1, rect.width)));
      const t = ratio * duration;
      if (kind === 'start') {
        const next = Math.min(t, trimEnd - 0.15);
        setTrimStart(next);
        seekTo(next);
      } else if (kind === 'end') {
        const next = Math.max(t, trimStart + 0.15);
        setTrimEnd(next);
        seekTo(next);
      } else {
        seekTo(t);
      }
    };
    const onUp = () => {
      dragRef.current = null;
    };
    window.addEventListener('pointermove', onMove);
    window.addEventListener('pointerup', onUp);
    return () => {
      window.removeEventListener('pointermove', onMove);
      window.removeEventListener('pointerup', onUp);
    };
  }, [duration, trimStart, trimEnd, seekTo]);

  const startScrub = (e: React.PointerEvent<HTMLDivElement>) => {
    const rect = e.currentTarget.getBoundingClientRect();
    const ratio = Math.max(0, Math.min(1, (e.clientX - rect.left) / Math.max(1, rect.width)));
    dragRef.current = 'scrub';
    seekTo(ratio * duration);
  };

  const trackCount = Math.max(1, Math.min(3, clip.audio_tracks || 1));
  const visibleTracks = useMemo(
    () => Array.from({ length: trackCount }, (_, idx) => idx),
    [trackCount],
  );
  const trimStartPct = (trimStart / duration) * 100;
  const trimEndPct = (trimEnd / duration) * 100;
  const playheadPct = Math.max(0, Math.min(100, (currentTime / duration) * 100));
  const trimmedSpan = Math.max(0, trimEnd - trimStart);
  const hasAudioMixChange = visibleTracks.some(
    (idx) => Boolean(mutedTracks[idx]) || Math.abs((trackVolumes[idx] ?? 1) - 1) > 0.005,
  );
  const hasTrimChange = trimStart > 0.05 || trimEnd < duration - 0.05 || hasAudioMixChange;

  const toggleTrackMute = (idx: number) => {
    const isMuted = Boolean(mutedTracks[idx]);
    const curVol = trackVolumes[idx] ?? 1;
    if (isMuted) {
      setMutedTracks((prev) => prev.map((m, i) => (i === idx ? false : m)));
      if (curVol <= 0.001) {
        setTrackVolumes((prev) => prev.map((v, i) => (i === idx ? 1 : v)));
      }
    } else if (curVol <= 0.001) {
      setTrackVolumes((prev) => prev.map((v, i) => (i === idx ? 1 : v)));
    } else {
      setMutedTracks((prev) => prev.map((m, i) => (i === idx ? true : m)));
    }
  };

  const setTrackVolume = (idx: number, nextVol: number) => {
    const clamped = Math.max(0, Math.min(1, nextVol));
    setTrackVolumes((prev) => prev.map((v, i) => (i === idx ? clamped : v)));
    if (clamped > 0.001 && mutedTracks[idx]) {
      setMutedTracks((prev) => prev.map((m, i) => (i === idx ? false : m)));
    }
  };

  const handleSaveDetails = async () => {
    await onSaveDetails(clip.id, title, tag, description);
    setDetailsSaved(true);
    window.setTimeout(() => setDetailsSaved(false), 1600);
  };

  const handleCopy = async () => {
    await onCopyClip(clip.path);
    setCopiedFeedback(true);
    window.setTimeout(() => setCopiedFeedback(false), 1800);
  };

  const handleSafeDelete = async () => {
    const v = videoRef.current;
    if (v) {
      v.pause();
      v.removeAttribute('src');
      v.load();
    }
    for (const a of audioRefs.current) {
      if (a) {
        a.pause();
        a.removeAttribute('src');
        a.load();
      }
    }
    await onDeleteClip(clip);
  };

  const frameStep = 1 / Math.max(15, clip.fps || 60);

  const rulerTicks = useMemo(() => {
    const dur = Math.max(0.5, duration);
    let majorStep = 10;
    let minorStep = 2;
    if (dur <= 12) {
      majorStep = 2;
      minorStep = 0.5;
    } else if (dur <= 35) {
      majorStep = 5;
      minorStep = 1;
    } else if (dur <= 90) {
      majorStep = 10;
      minorStep = 2;
    } else {
      majorStep = 30;
      minorStep = 5;
    }
    const ticks: { pct: number; major: boolean; label?: string }[] = [];
    const count = Math.floor(dur / minorStep);
    for (let i = 0; i <= count; i++) {
      const t = +(i * minorStep).toFixed(3);
      const pct = (t / dur) * 100;
      if (pct > 98.5) continue;
      const rem = t % majorStep;
      const isMajor = Math.abs(rem) < 0.01 || Math.abs(rem - majorStep) < 0.01;
      let label: string | undefined;
      if (isMajor) {
        const totalSec = Math.round(t);
        const m = Math.floor(totalSec / 60);
        const s = totalSec % 60;
        label = `${m}:${String(s).padStart(2, '0')}`;
      }
      ticks.push({ pct, major: isMajor, label });
    }
    return ticks;
  }, [duration]);

  return (
    <div className="editor-shell">
      <div className="editor-meta-bar">
        <div className="editor-meta-left">
          <button className="btn-ghost" onClick={onBack}>
            <ArrowLeft size={15} />
            All replays
          </button>
          <span className="editor-divider" aria-hidden="true" />
          <span style={{ fontWeight: 700 }}>{clip.game}</span>
          <span className="mono" style={{ color: 'var(--ink-engraved)', fontSize: 11 }}>
            {formatDateTime(clip.created_at)}
          </span>
          <span className="meta-chip mono">
            {clip.width}×{clip.height} · {Math.round(clip.fps)}fps · {formatBytes(clip.size_bytes)}
          </span>
        </div>

        <div className="editor-meta-right">
          <button
            className="path-link mono"
            onClick={() => onRevealClip(clip.path)}
            title={`Reveal in Explorer: ${clip.path}`}
          >
            {clip.path}
          </button>
          <button className="btn-icon" onClick={onBack} title="Close Editor">
            <X size={15} />
          </button>
        </div>
      </div>

      <div className="editor-stage-split">
        <section className="video-stage">
          <div className="monitor-chassis">
            <div className="video-frame-box">
              <video
                ref={videoRef}
                src={mediaUrl(clip.path)}
                preload="auto"
                onClick={togglePlay}
                onLoadedMetadata={(e) => {
                  const vd = e.currentTarget.duration;
                  if (Number.isFinite(vd) && vd > 0) {
                    setDuration(vd);
                    setTrimEnd((cur) => (cur === initialDur ? vd : Math.min(cur, vd)));
                  }
                  syncAudioElements(true);
                }}
                onPlay={() => {
                  setPlaying(true);
                  syncAudioElements(false);
                }}
                onPause={() => {
                  setPlaying(false);
                  syncAudioElements(false);
                }}
                onEnded={() => {
                  setPlaying(false);
                  syncAudioElements(false);
                }}
                onSeeked={() => syncAudioElements(true)}
              />

              {trackAudioUrls.map((url, idx) => (
                <audio
                  key={url}
                  ref={(el) => {
                    audioRefs.current[idx] = el;
                  }}
                  src={url}
                  preload="auto"
                  onLoadedMetadata={() => syncAudioElements(true)}
                />
              ))}

              {canvasMode === '9:16' && (
                <div className="vertical-overlay-guide">
                  <div className="vertical-crop-window">
                    <span className="vertical-crop-badge">
                      9:16 {blurBg ? 'Blurred Backdrop' : 'Center Crop'}
                    </span>
                  </div>
                </div>
              )}
              {canvasMode === 'custom' && <CropOverlay crop={crop} onChange={setCrop} aspect={clip.width / Math.max(1, clip.height)} />}
            </div>
            <div className="monitor-chin mono">
              <span className="monitor-tally">
                <span className={`monitor-tally-led ${playing ? 'playing' : ''}`} />
                {clip.title}
              </span>
              <span>
                {clip.width}×{clip.height} · {Math.round(clip.fps)} FPS
              </span>
            </div>
          </div>
        </section>

        <aside className="inspector-pane">
          <div className="inspector-section">
            <span className="inspector-label">Export format</span>
            <div className="canvas-toggle">
              <button
                className={`canvas-btn ${canvasMode === 'original' ? 'active' : ''}`}
                aria-pressed={canvasMode === 'original'}
                onClick={() => setCanvasMode('original')}
              >
                Original
              </button>
              <button
                className={`canvas-btn ${canvasMode === '9:16' ? 'active' : ''}`}
                aria-pressed={canvasMode === '9:16'}
                onClick={() => setCanvasMode('9:16')}
              >
                9:16 vertical
              </button>
              <button className={`canvas-btn ${canvasMode === 'custom' ? 'active' : ''}`} aria-pressed={canvasMode === 'custom'} onClick={() => setCanvasMode('custom')}>Custom</button>
            </div>
            {canvasMode === 'custom' && <>
              <CropControls crop={crop} onChange={setCrop} width={clip.width} height={clip.height} />
              <button className="btn-primary" disabled={busyAction === 'export916'} onClick={() => onExportVertical(clip.id, false, trimStart, trimEnd, mutedTracks.slice(0, trackCount), trackVolumes.slice(0, trackCount), crop)}><Download size={14} />{busyAction === 'export916' ? 'Exporting…' : 'Export cropped MP4'}</button>
            </>}

            {canvasMode === '9:16' && (
              <div style={{ display: 'flex', flexDirection: 'column', gap: 8, marginTop: 4 }}>
                <div className="canvas-toggle">
                  <button
                    className={`canvas-btn ${blurBg ? 'active' : ''}`}
                    style={{ height: 30, fontSize: 12 }}
                    onClick={() => setBlurBg(true)}
                  >
                    Blurred Bg
                  </button>
                  <button
                    className={`canvas-btn ${!blurBg ? 'active' : ''}`}
                    style={{ height: 30, fontSize: 12 }}
                    onClick={() => setBlurBg(false)}
                  >
                    Center Crop
                  </button>
                </div>
                <button
                  className="btn-primary"
                  disabled={busyAction === 'export916'}
                  onClick={() =>
                    onExportVertical(
                      clip.id,
                      blurBg,
                      trimStart,
                      trimEnd,
                      mutedTracks.slice(0, trackCount),
                      trackVolumes.slice(0, trackCount),
                    )
                  }
                >
                  <Download size={14} />
                  {busyAction === 'export916' ? 'Exporting 9:16...' : 'Export 9:16 MP4'}
                </button>
              </div>
            )}
          </div>

          <div className="inspector-section">
            <span className="inspector-label">Clip details</span>
            <div className="field-group">
              <label htmlFor="clip-title">Title</label>
              <input
                type="text" id="clip-title"
                value={title}
                onChange={(e) => setTitle(e.target.value)}
                onBlur={handleSaveDetails}
              />
            </div>

            <div className="field-group">
              <label htmlFor="clip-tag">Tag</label>
              <input
                type="text"
                id="clip-tag" placeholder="e.g. Clutch, Ace, Flick"
                value={tag}
                onChange={(e) => setTag(e.target.value)}
                onBlur={handleSaveDetails}
              />
            </div>

            <div className="field-group">
              <label htmlFor="clip-notes">Notes</label>
              <textarea
                rows={2} id="clip-notes"
                placeholder="Add description or timestamp notes..."
                value={description}
                onChange={(e) => setDescription(e.target.value)}
                onBlur={handleSaveDetails}
              />
            </div>

            <button className="btn-secondary" onClick={handleSaveDetails}>
              {detailsSaved ? <Check size={14} /> : <Sparkles size={14} />}
              {detailsSaved ? 'Saved Details' : 'Save Details'}
            </button>
          </div>

          <div className="inspector-section" style={{ marginTop: 'auto' }}>
            <span className="inspector-label">Clip file</span>
            <button className="btn-secondary" onClick={() => onRevealClip(clip.path)}>
              <FolderOpen size={14} />
              Show in Explorer
            </button>
            {confirmDelete ? (
              <div style={{ display: 'flex', gap: 6 }}>
                <button
                  className="btn-secondary"
                  style={{ flex: 1 }}
                  onClick={() => setConfirmDelete(false)}
                >
                  Cancel
                </button>
                <button
                  className="btn-danger"
                  style={{ flex: 1 }}
                  onClick={handleSafeDelete}
                >
                  Confirm Delete
                </button>
              </div>
            ) : (
              <button className="btn-danger" onClick={() => setConfirmDelete(true)}>
                <Trash2 size={14} />
                Delete Clip
              </button>
            )}
          </div>
        </aside>
      </div>

      <section className="timeline-deck">
        <div className="transport-bar">
          <div className="transport-left">
            <button
              className="btn-icon"
              onClick={() => seekTo(trimStart)}
              title="Jump to Trim Start"
            >
              <SkipBack size={14} />
            </button>
            <button
              className="btn-icon"
              onClick={() => seekTo(currentTime - frameStep)}
              title="Previous Frame (Left Arrow)"
            >
              <ChevronLeft size={15} />
            </button>
            <div className="play-btn-collar">
              <button
                className="play-btn"
                onClick={togglePlay}
                title="Play / Pause (Space)"
              >
                {playing ? (
                  <Pause size={15} fill="currentColor" />
                ) : (
                  <Play size={15} fill="currentColor" style={{ marginLeft: 2 }} />
                )}
              </button>
            </div>
            <button
              className="btn-icon"
              onClick={() => seekTo(currentTime + frameStep)}
              title="Next Frame (Right Arrow)"
            >
              <ChevronRight size={15} />
            </button>
            <button
              className="btn-icon"
              onClick={() => seekTo(trimEnd)}
              title="Jump to Trim End"
            >
              <SkipForward size={14} />
            </button>

            <span className="timecode-lcd mono">
              {formatTimecode(currentTime)}{' '}
              <span className="total">/ {formatTimecode(duration)}</span>
            </span>

            <div className="volume-wrap" title={`Master preview volume: ${Math.round(volume * 100)}%`}>
              <Volume2 size={14} />
              <input
                type="range"
                min={0}
                max={1}
                step={0.02}
                value={volume}
                onChange={(e) => setVolume(Number(e.target.value))}
                aria-label="Preview volume"
              />
            </div>

            <span className="meta-chip mono" title="Selected trim range">
              Trim: {formatTimecode(trimStart)} – {formatTimecode(trimEnd)} ({trimmedSpan.toFixed(1)}s)
            </span>
          </div>

          <div className="transport-right">
            <button
              className="btn-secondary"
              disabled={!hasTrimChange || busyAction === 'trim'}
              onClick={() =>
                onTrimClip(
                  clip.id,
                  trimStart,
                  trimEnd,
                  mutedTracks.slice(0, trackCount),
                  trackVolumes.slice(0, trackCount),
                )
              }
              title="Save trimmed range & track audio levels to a new MP4 clip"
            >
              <Scissors size={14} />
              {busyAction === 'trim' ? 'Trimming...' : 'Save Trim'}
            </button>

            <button
              className="btn-primary"
              onClick={handleCopy}
              title="Copy MP4 file to Windows clipboard (ready to paste in Discord)"
            >
              {copiedFeedback ? <Check size={14} /> : <ClipboardCopy size={14} />}
              {copiedFeedback ? 'Copied to Clipboard!' : 'Share Clip'}
            </button>
          </div>
        </div>

        <div className="tracks-board">
          {/* Ruler and lanes share one horizontal coordinate system. */}
          <div className="timeline-ruler-row">
            <div className="timeline-ruler-spacer">Timeline</div>
            <div className="timeline-ruler" onPointerDown={startScrub} title="Click or drag to scrub">
              {rulerTicks.map((tk, idx) => (
                <div key={idx}>
                  <span
                    className={`ruler-tick ${tk.major ? 'major' : 'minor'}`}
                    style={{ left: `${tk.pct}%` }}
                  />
                  {tk.label && (
                    <span className="ruler-label mono" style={{ left: `${tk.pct}%` }}>
                      {tk.label}
                    </span>
                  )}
                </div>
              ))}
            </div>
          </div>

          {/* Video Track + Trim Brackets */}
          <div className="track-row video-lane">
            <div className="track-label-col">
              <span className="track-name">
                <span className="track-swatch" style={{ background: 'var(--game-track)' }} />
                Video ({Math.round(clip.fps)}fps)
              </span>
              <span className="mono" style={{ fontSize: 10, color: 'var(--ink-engraved)' }}>
                {clip.height}p
              </span>
            </div>

            <div
              ref={videoLaneRef}
              className="track-lane"
              onPointerDown={startScrub}
              title="Drag handles to trim (keys I / O set start & end)"
            >
              <div className="trim-mask">
                <div className="trim-dim-left" style={{ left: 0, width: `${trimStartPct}%` }} />
                <div
                  className="trim-selection-box"
                  style={{ left: `${trimStartPct}%`, width: `${trimEndPct - trimStartPct}%` }}
                />
                <div
                  className="trim-dim-right"
                  style={{ left: `${trimEndPct}%`, width: `${100 - trimEndPct}%` }}
                />
              </div>

              <div
                className="trim-handle"
                style={{ left: `${trimStartPct}%` }}
                onPointerDown={(e) => {
                  e.stopPropagation();
                  dragRef.current = 'start';
                }}
                title="Drag to set trim start"
              />
              <div
                className="trim-handle"
                style={{ left: `${trimEndPct}%` }}
                onPointerDown={(e) => {
                  e.stopPropagation();
                  dragRef.current = 'end';
                }}
                title="Drag to set trim end"
              />

              <div className="playhead-line" style={{ left: `${playheadPct}%` }} />
            </div>
          </div>

          {/* Audio Tracks with Waveforms, Per-Track Volume & Mute */}
          {visibleTracks.map((idx) => {
            const meta = TRACK_META[idx];
            const Icon = meta.icon;
            const isMuted = Boolean(mutedTracks[idx]);
            const trackVol = trackVolumes[idx] ?? 1;
            const volPct = Math.round(trackVol * 100);
            const isEffectivelyMuted = isMuted || trackVol <= 0.001;
            const sliderStyle = {
              '--track-color': isEffectivelyMuted ? 'var(--ink-dim)' : meta.color,
              '--volume-pct': `${volPct}%`,
            } as React.CSSProperties;

            return (
              <div className="track-row" key={meta.label}>
                <div className="track-label-col audio-track-controls">
                  <div className="track-header-top">
                    <span className="track-name">
                      <span
                        className="track-swatch"
                        style={{ background: meta.color, color: meta.color }}
                      />
                      <Icon size={12} color={meta.color} />
                      {meta.label}
                    </span>
                    <button
                      type="button"
                      className={`mute-toggle ${isEffectivelyMuted ? 'muted' : ''}`}
                      onClick={() => toggleTrackMute(idx)}
                      title={isEffectivelyMuted ? `Unmute ${meta.label}` : `Mute ${meta.label}`}
                      aria-label={isEffectivelyMuted ? `Unmute ${meta.label}` : `Mute ${meta.label}`}
                    >
                      {isEffectivelyMuted ? (
                        <VolumeX size={11} />
                      ) : trackVol < 0.5 ? (
                        <Volume1 size={11} />
                      ) : (
                        <Volume2 size={11} />
                      )}
                    </button>
                  </div>

                  <div className="track-volume-row">
                    <input
                      type="range"
                      className="track-volume-slider"
                      min={0}
                      max={1}
                      step={0.01}
                      value={trackVol}
                      style={sliderStyle}
                      onChange={(e) => setTrackVolume(idx, Number(e.target.value))}
                      onDoubleClick={() => setTrackVolume(idx, 1)}
                      title={`${meta.label} volume: ${volPct}% (double-click to reset)`}
                      aria-label={`${meta.label} volume`}
                    />
                    <span
                      className={`track-volume-pct mono ${isEffectivelyMuted ? 'muted' : ''}`}
                      title={isMuted ? `Muted (${volPct}%)` : `${volPct}%`}
                    >
                      {isMuted ? 'MUTE' : `${volPct}%`}
                    </span>
                  </div>
                </div>

                <div className="track-lane" onPointerDown={startScrub}>
                  <WaveformCanvas
                    theme={theme}
                    peaks={waveforms[idx] ?? []}
                    color={meta.color}
                    muted={isMuted}
                    volume={trackVol}
                    trimStartRatio={trimStart / duration}
                    trimEndRatio={trimEnd / duration}
                  />
                  <div className="playhead-line" style={{ left: `${playheadPct}%` }} />
                </div>
              </div>
            );
          })}
        </div>
        <p className="timeline-help">Space to play · I / O to set trim · Arrow keys to step through frames</p>
      </section>
    </div>
  );
}
