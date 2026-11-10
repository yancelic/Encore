import { useMemo, useState } from 'react';
import {
  Check,
  ClipboardCopy,
  Film,
  FolderOpen,
  Layers,
  LayoutGrid,
  List,
  Play,
  Search,
  Star,
  Trash2,
  Video,
} from 'lucide-react';
import {
  dateGroupLabel,
  formatBytes,
  formatDuration,
  formatRelativeAge,
  mediaUrl,
} from '../api';
import type { ClipRecord, SortOrder } from '../types';

interface LibraryViewProps {
  clips: ClipRecord[];
  allGames: string[];
  loading: boolean;
  search: string;
  gameFilter: string | null;
  favoritesOnly: boolean;
  sortOrder: SortOrder;
  hotkey: string;
  clipsDir: string;
  onSearchChange: (s: string) => void;
  onGameFilterChange: (g: string | null) => void;
  onFavoritesOnlyChange: (f: boolean) => void;
  onSortOrderChange: (s: SortOrder) => void;
  onOpenClip: (clip: ClipRecord) => void;
  onToggleFavorite: (clip: ClipRecord) => void;
  onCopyClip: (clip: ClipRecord) => void;
  onRevealClip: (clip: ClipRecord) => void;
  onDeleteClip: (clip: ClipRecord) => void;
  onCreateMontage: (selected: ClipRecord[]) => void;
}

interface ClipGroup {
  key: string;
  label: string;
  items: ClipRecord[];
}

export function LibraryView({
  clips,
  allGames,
  loading,
  search,
  gameFilter,
  favoritesOnly,
  sortOrder,
  hotkey,
  clipsDir,
  onSearchChange,
  onGameFilterChange,
  onFavoritesOnlyChange,
  onSortOrderChange,
  onOpenClip,
  onToggleFavorite,
  onCopyClip,
  onRevealClip,
  onDeleteClip,
  onCreateMontage,
}: LibraryViewProps) {
  const [viewMode, setViewMode] = useState<'grid' | 'list'>('grid');
  const [montageMode, setMontageMode] = useState(false);
  const [selectedIds, setSelectedIds] = useState<string[]>([]);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);

  const groups: ClipGroup[] = useMemo(() => {
    if (sortOrder === 'Longest') {
      return clips.length ? [{ key: 'longest', label: 'Longest Clips First', items: clips }] : [];
    }
    if (sortOrder === 'FavoritesFirst') {
      const favs = clips.filter((c) => c.favorite);
      const rest = clips.filter((c) => !c.favorite);
      const out: ClipGroup[] = [];
      if (favs.length) out.push({ key: 'favs', label: 'Favorites', items: favs });
      if (rest.length) out.push({ key: 'others', label: 'Other Clips', items: rest });
      return out;
    }

    const map = new Map<string, ClipGroup>();
    for (const c of clips) {
      const dateLbl = dateGroupLabel(c.created_at);
      const key = `${dateLbl}::${c.game}`;
      const existing = map.get(key);
      if (existing) {
        existing.items.push(c);
      } else {
        map.set(key, { key, label: `${dateLbl} · ${c.game}`, items: [c] });
      }
    }
    return Array.from(map.values());
  }, [clips, sortOrder]);

  const selectedClips = useMemo(
    () =>
      selectedIds
        .map((id) => clips.find((c) => c.id === id))
        .filter((c): c is ClipRecord => Boolean(c)),
    [selectedIds, clips],
  );

  const toggleMontagePick = (id: string) => {
    setSelectedIds((prev) =>
      prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id],
    );
  };

  const handleCopy = (e: React.MouseEvent, clip: ClipRecord) => {
    e.stopPropagation();
    onCopyClip(clip);
    setCopiedId(clip.id);
    window.setTimeout(() => {
      setCopiedId((cur) => (cur === clip.id ? null : cur));
    }, 1800);
  };

  const handleCardClick = (clip: ClipRecord) => {
    if (montageMode) {
      toggleMontagePick(clip.id);
    } else {
      onOpenClip(clip);
    }
  };

  return (
    <main className="library-main">
      <div className="library-toolbar">
        <div className="toolbar-left">
          <span className="section-heading">ALL CLIPS ({clips.length})</span>

          <button
            className={`filter-pill ${favoritesOnly ? 'active' : ''}`}
            onClick={() => onFavoritesOnlyChange(!favoritesOnly)}
            title="Show starred clips only"
          >
            <Star size={13} fill={favoritesOnly ? 'currentColor' : 'none'} />
            Favorites
          </button>

          <button
            className={montageMode ? 'btn-secondary' : 'btn-primary'}
            onClick={() => {
              setMontageMode((v) => !v);
              setSelectedIds([]);
            }}
          >
            <Layers size={14} />
            {montageMode ? 'Cancel Montage' : 'Create Montage'}
          </button>
        </div>

        <div className="toolbar-right">
          <div className="search-box">
            <Search size={14} />
            <input
              type="text"
              placeholder="Search title, game, tag..."
              value={search}
              onChange={(e) => onSearchChange(e.target.value)}
            />
          </div>

          <select
            className="toolbar-select"
            value={gameFilter ?? ''}
            onChange={(e) => onGameFilterChange(e.target.value || null)}
            aria-label="Filter by game"
          >
            <option value="">All Games</option>
            {allGames.map((g) => (
              <option key={g} value={g}>
                {g}
              </option>
            ))}
          </select>

          <select
            className="toolbar-select"
            value={sortOrder}
            onChange={(e) => onSortOrderChange(e.target.value as SortOrder)}
            aria-label="Sort clips"
          >
            <option value="Newest">Newest First</option>
            <option value="Oldest">Oldest First</option>
            <option value="Longest">Longest First</option>
            <option value="FavoritesFirst">Favorites First</option>
          </select>

          <div className="mode-switch">
            <button
              className={viewMode === 'grid' ? 'active' : ''}
              onClick={() => setViewMode('grid')}
              title="Grid view"
            >
              <LayoutGrid size={14} />
            </button>
            <button
              className={viewMode === 'list' ? 'active' : ''}
              onClick={() => setViewMode('list')}
              title="List view"
            >
              <List size={14} />
            </button>
          </div>
        </div>
      </div>

      {montageMode && (
        <div className="montage-bar">
          <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
            <Layers size={16} color="#7D91FF" />
            <span>
              Click clips in the order you want them stitched together.{' '}
              <strong>{selectedClips.length}</strong> selected.
            </span>
          </div>
          <div style={{ display: 'flex', gap: 8 }}>
            <button
              className="btn-ghost"
              onClick={() => setSelectedIds([])}
              disabled={selectedClips.length === 0}
            >
              Clear
            </button>
            <button
              className="btn-primary"
              disabled={selectedClips.length < 2}
              onClick={() => onCreateMontage(selectedClips)}
            >
              Stitch {selectedClips.length} Clips
            </button>
          </div>
        </div>
      )}

      <div className="library-scroll">
        {loading && clips.length === 0 ? (
          <div className="empty-state">
            <Film size={28} color="#9FA9BE" />
            <h3>Scanning your Clips folder...</h3>
            <p className="mono" style={{ fontSize: 11 }}>{clipsDir}</p>
          </div>
        ) : clips.length === 0 ? (
          <div className="empty-state">
            <Video size={32} color="#6F84FF" />
            <h3>No clips found yet</h3>
            <p>
              Press <span className="mono">{hotkey}</span> while playing to save the last seconds
              instantly, or drop MP4 videos into your Clips folder.
            </p>
            <span className="mono" style={{ fontSize: 11, color: '#9FA9BE' }}>
              {clipsDir}
            </span>
          </div>
        ) : (
          groups.map((grp) => (
            <section className="clip-group" key={grp.key}>
              <div className="clip-group-header">
                <span className="clip-group-title">{grp.label}</span>
                <span className="clip-group-count mono">{grp.items.length}</span>
                <span className="clip-group-rule" />
              </div>

              {viewMode === 'grid' ? (
                <div className="clips-grid">
                  {grp.items.map((clip) => {
                    const pickIdx = selectedIds.indexOf(clip.id);
                    const isPicked = pickIdx !== -1;
                    const isCopied = copiedId === clip.id;
                    const confirming = confirmDeleteId === clip.id;
                    return (
                      <article
                        key={clip.id}
                        className={`clip-card ${isPicked ? 'selected' : ''}`}
                        onClick={() => handleCardClick(clip)}
                      >
                        <div className="clip-thumb-wrap">
                          {clip.thumb_path ? (
                            <img
                              className="clip-thumb"
                              src={mediaUrl(clip.thumb_path)}
                              alt={clip.title}
                              loading="lazy"
                            />
                          ) : (
                            <div className="clip-thumb-fallback">
                              <Film size={28} />
                            </div>
                          )}

                          {!montageMode && (
                            <div className="thumb-hover-play">
                              <div className="play-circle">
                                <Play size={18} fill="currentColor" style={{ marginLeft: 2 }} />
                              </div>
                            </div>
                          )}

                          <div className="thumb-top-left">
                            {montageMode ? (
                              <span className={`montage-order-badge ${isPicked ? 'picked' : ''}`}>
                                {isPicked ? pickIdx + 1 : '+'}
                              </span>
                            ) : (
                              <span className="track-badge mono">
                                {clip.height > 0 ? `${clip.height}p` : ''}
                                {clip.fps > 0 ? ` ${Math.round(clip.fps)}` : ''}
                                <span className="track-dots" title={`${clip.audio_tracks} audio track(s)`}>
                                  <span style={{ background: '#16C9B3', color: '#16C9B3' }} />
                                  {clip.audio_tracks >= 2 && <span style={{ background: '#3B82F6', color: '#3B82F6' }} />}
                                  {clip.audio_tracks >= 3 && <span style={{ background: '#F59E0B', color: '#F59E0B' }} />}
                                </span>
                              </span>
                            )}
                          </div>

                          <button
                            className={`fav-btn ${clip.favorite ? 'is-fav' : ''}`}
                            title={clip.favorite ? 'Unfavorite' : 'Favorite'}
                            onClick={(e) => {
                              e.stopPropagation();
                              onToggleFavorite(clip);
                            }}
                          >
                            <Star size={14} fill={clip.favorite ? 'currentColor' : 'none'} />
                          </button>

                          <span className="duration-badge mono">
                            {formatDuration(clip.duration_ms)}
                          </span>
                        </div>

                        <div className="clip-body">
                          <div className="clip-title-row">
                            <span className="clip-title" title={clip.title}>
                              {clip.title}
                            </span>
                            {clip.tag && <span className="clip-tag">{clip.tag}</span>}
                          </div>

                          <div className="clip-meta-row">
                            <span className="mono">
                              {formatRelativeAge(clip.created_at)} · {formatBytes(clip.size_bytes)}
                            </span>

                            <div className="clip-actions" onClick={(e) => e.stopPropagation()}>
                              <button
                                className={`btn-chip ${isCopied ? 'copied' : ''}`}
                                onClick={(e) => handleCopy(e, clip)}
                                title="Copy MP4 file to Windows clipboard (paste into Discord)"
                              >
                                {isCopied ? <Check size={12} /> : <ClipboardCopy size={12} />}
                                {isCopied ? 'Copied' : 'Copy'}
                              </button>

                              <button
                                className="btn-chip"
                                onClick={() => onRevealClip(clip)}
                                title="Show in Explorer"
                              >
                                <FolderOpen size={12} />
                              </button>

                              {confirming ? (
                                <button
                                  className="btn-danger"
                                  style={{ height: 25, padding: '0 7px', fontSize: 11 }}
                                  onClick={() => {
                                    setConfirmDeleteId(null);
                                    onDeleteClip(clip);
                                  }}
                                >
                                  Delete?
                                </button>
                              ) : (
                                <button
                                  className="btn-chip"
                                  onClick={() => setConfirmDeleteId(clip.id)}
                                  title="Delete clip"
                                >
                                  <Trash2 size={12} />
                                </button>
                              )}
                            </div>
                          </div>
                        </div>
                      </article>
                    );
                  })}
                </div>
              ) : (
                <div className="clips-list">
                  {grp.items.map((clip) => {
                    const pickIdx = selectedIds.indexOf(clip.id);
                    const isPicked = pickIdx !== -1;
                    const isCopied = copiedId === clip.id;
                    const confirming = confirmDeleteId === clip.id;
                    return (
                      <div
                        key={clip.id}
                        className={`clip-row ${isPicked ? 'selected' : ''}`}
                        onClick={() => handleCardClick(clip)}
                      >
                        {montageMode && (
                          <span className={`montage-order-badge ${isPicked ? 'picked' : ''}`}>
                            {isPicked ? pickIdx + 1 : '+'}
                          </span>
                        )}
                        {clip.thumb_path ? (
                          <img
                            className="clip-row-thumb"
                            src={mediaUrl(clip.thumb_path)}
                            alt={clip.title}
                          />
                        ) : (
                          <div className="clip-row-thumb clip-thumb-fallback">
                            <Film size={18} />
                          </div>
                        )}
                        <div style={{ flex: 1, minWidth: 0 }}>
                          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                            <span className="clip-title">{clip.title}</span>
                            {clip.tag && <span className="clip-tag">{clip.tag}</span>}
                          </div>
                          <div className="mono" style={{ color: 'var(--ink-engraved)', fontSize: 11, marginTop: 2 }}>
                            {clip.game} · {formatDuration(clip.duration_ms)} · {clip.width}×{clip.height} ·{' '}
                            {Math.round(clip.fps)}fps · {formatBytes(clip.size_bytes)} ·{' '}
                            {formatRelativeAge(clip.created_at)}
                          </div>
                        </div>

                        <div className="clip-actions" onClick={(e) => e.stopPropagation()}>
                          <button
                            className="btn-chip"
                            style={{ color: clip.favorite ? '#F59E0B' : undefined }}
                            onClick={() => onToggleFavorite(clip)}
                            title="Favorite"
                          >
                            <Star size={12} fill={clip.favorite ? 'currentColor' : 'none'} />
                          </button>
                          <button
                            className={`btn-chip ${isCopied ? 'copied' : ''}`}
                            onClick={(e) => handleCopy(e, clip)}
                          >
                            {isCopied ? <Check size={12} /> : <ClipboardCopy size={12} />}
                            {isCopied ? 'Copied' : 'Copy Clip'}
                          </button>
                          <button className="btn-chip" onClick={() => onRevealClip(clip)}>
                            <FolderOpen size={12} />
                          </button>
                          {confirming ? (
                            <button
                              className="btn-danger"
                              style={{ height: 25, padding: '0 8px', fontSize: 11 }}
                              onClick={() => {
                                setConfirmDeleteId(null);
                                onDeleteClip(clip);
                              }}
                            >
                              Delete?
                            </button>
                          ) : (
                            <button
                              className="btn-chip"
                              onClick={() => setConfirmDeleteId(clip.id)}
                            >
                              <Trash2 size={12} />
                            </button>
                          )}
                        </div>
                      </div>
                    );
                  })}
                </div>
              )}
            </section>
          ))
        )}
      </div>
    </main>
  );
}
