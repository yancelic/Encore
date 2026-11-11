import { useMemo, useState } from 'react';
import {
  Check,
  ClipboardCopy,
  Film,
  FolderOpen,
  Gamepad2,
  Layers,
  LayoutGrid,
  List,
  Play,
  Search,
  Star,
  Trash2,
  Video,
  X,
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
  onCopyClip: (clip: ClipRecord) => Promise<boolean>;
  onRevealClip: (clip: ClipRecord) => void;
  onDeleteClip: (clip: ClipRecord) => void;
  onCreateMontage: (selected: ClipRecord[]) => void;
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
  const [selectedClips, setSelectedClips] = useState<ClipRecord[]>([]);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [copyingId, setCopyingId] = useState<string | null>(null);
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);
  const filtered = Boolean(search || gameFilter || favoritesOnly);

  const groups = useMemo(() => {
    if (sortOrder === 'Longest')
      return clips.length
        ? [{ label: 'Longest clips first', items: clips }]
        : [];
    if (sortOrder === 'FavoritesFirst')
      return [
        { label: 'Favorites', items: clips.filter((c) => c.favorite) },
        { label: 'Other replays', items: clips.filter((c) => !c.favorite) },
      ].filter((g) => g.items.length);
    const map = new Map<string, ClipRecord[]>();
    for (const clip of clips) {
      const label = `${dateGroupLabel(clip.created_at)} · ${clip.game}`;
      const group = map.get(label);
      if (group) group.push(clip);
      else map.set(label, [clip]);
    }
    return Array.from(map, ([label, items]) => ({ label, items }));
  }, [clips, sortOrder]);

  const resetFilters = () => {
    onSearchChange('');
    onGameFilterChange(null);
    onFavoritesOnlyChange(false);
  };
  const openClip = (clip: ClipRecord) => {
    if (montageMode)
      setSelectedClips((selected) =>
        selected.some((item) => item.id === clip.id)
          ? selected.filter((item) => item.id !== clip.id)
          : [...selected, clip],
      );
    else onOpenClip(clip);
  };
  const copyClip = async (clip: ClipRecord) => {
    setCopyingId(clip.id);
    setCopiedId(null);
    try {
      if (await onCopyClip(clip)) {
        setCopiedId(clip.id);
        window.setTimeout(
          () => setCopiedId((id) => (id === clip.id ? null : id)),
          1800,
        );
      }
    } finally {
      setCopyingId(null);
    }
  };
  const renderActions = (clip: ClipRecord) => (
    <div className="clip-actions">
      <button
        className={`btn-chip ${copiedId === clip.id ? 'copied' : ''}`}
        disabled={copyingId !== null}
        onClick={() => copyClip(clip)}
        title="Copy clip file to clipboard"
      >
        {copiedId === clip.id ? (
          <Check size={14} />
        ) : (
          <ClipboardCopy size={14} />
        )}
        <span>{copyingId === clip.id ? 'Preparing...' : copiedId === clip.id ? 'Copied' : 'Copy'}</span>
      </button>
      <button
        className="btn-chip"
        onClick={() => onRevealClip(clip)}
        title="Show in Explorer"
        aria-label={`Show ${clip.title} in Explorer`}
      >
        <FolderOpen size={14} />
      </button>
      {confirmDeleteId === clip.id ? (
        <>
          <button
            className="btn-danger btn-compact"
            onClick={() => {
              setConfirmDeleteId(null);
              setSelectedClips((selected) =>
                selected.filter((item) => item.id !== clip.id),
              );
              onDeleteClip(clip);
            }}
          >
            Delete?
          </button>
          <button
            className="btn-chip"
            onClick={() => setConfirmDeleteId(null)}
            aria-label="Cancel deletion"
          >
            <X size={14} />
          </button>
        </>
      ) : (
        <button
          className="btn-chip"
          onClick={() => setConfirmDeleteId(clip.id)}
          title="Delete clip"
          aria-label={`Delete ${clip.title}`}
        >
          <Trash2 size={14} />
        </button>
      )}
    </div>
  );

  return (
    <main className="library-main">
      <aside className="library-sidebar" aria-label="Library navigation">
        <div className="sidebar-title">Your library</div>
        <nav className="library-nav" aria-label="Collections">
          <button
            className={`nav-item ${!favoritesOnly && !gameFilter ? 'active' : ''}`}
            onClick={resetFilters}
            aria-pressed={!favoritesOnly && !gameFilter}
          >
            <Film size={17} /> All replays
          </button>
          <button
            className={`nav-item ${favoritesOnly ? 'active' : ''}`}
            onClick={() => {
              onFavoritesOnlyChange(!favoritesOnly);
              onGameFilterChange(null);
            }}
            aria-pressed={favoritesOnly}
          >
            <Star size={17} /> Favorites
          </button>
        </nav>
        <div className="sidebar-games">
          <div className="sidebar-label">Games</div>
          {allGames.length ? (
            allGames.map((game) => (
              <button
                key={game}
                className={`nav-item game-nav ${gameFilter === game ? 'active' : ''}`}
                onClick={() =>
                  onGameFilterChange(gameFilter === game ? null : game)
                }
                aria-pressed={gameFilter === game}
                title={game}
              >
                <Gamepad2 size={16} />
                <span>{game}</span>
              </button>
            ))
          ) : (
            <p className="sidebar-hint">
              Your games appear here after your first capture.
            </p>
          )}
        </div>
        <div className="sidebar-bottom">
          <span className="sidebar-replay-mark" aria-hidden="true">
            ↶
          </span>
          <p>
            Good games
            <br />
            are worth keeping.
          </p>
          <div className="storage-note" title={clipsDir}>
            <FolderOpen size={14} />
            <span>Saved on this PC</span>
          </div>
        </div>
      </aside>

      <div className="library-content">
        <div className="library-heading">
          <div>
            <h1>
              {favoritesOnly
                ? 'Your favorites.'
                : gameFilter
                  ? `${gameFilter} replays.`
                  : 'Your replays.'}
            </h1>
            <p>
              {clips.length} {clips.length === 1 ? 'moment' : 'moments'}{' '}
              {filtered ? 'in this view' : 'worth another watch'}.
            </p>
          </div>
          <button
            className={montageMode ? 'btn-secondary' : 'btn-primary'}
            onClick={() => {
              setMontageMode((v) => !v);
              setSelectedClips([]);
            }}
            aria-pressed={montageMode}
          >
            <Layers size={16} />
            {montageMode ? 'Cancel montage' : 'Create montage'}
          </button>
        </div>
        <div className="library-toolbar">
          <div className="search-box">
            <Search size={16} />
            <input
              type="search"
              placeholder="Find a replay…"
              aria-label="Search clips"
              value={search}
              onChange={(e) => onSearchChange(e.target.value)}
            />
          </div>
          <div className="toolbar-right">
            {filtered && (
              <button className="btn-ghost btn-compact" onClick={resetFilters}>
                <X size={13} /> Clear filters
              </button>
            )}
            <select
              className="toolbar-select"
              value={sortOrder}
              onChange={(e) => onSortOrderChange(e.target.value as SortOrder)}
              aria-label="Sort clips"
            >
              <option value="Newest">Newest first</option>
              <option value="Oldest">Oldest first</option>
              <option value="Longest">Longest first</option>
              <option value="FavoritesFirst">Favorites first</option>
            </select>
            <div
              className="mode-switch view-switch"
              role="group"
              aria-label="Library view"
            >
              <button
                className={viewMode === 'grid' ? 'active' : ''}
                onClick={() => setViewMode('grid')}
                title="Grid view"
                aria-label="Grid view"
                aria-pressed={viewMode === 'grid'}
              >
                <LayoutGrid size={16} />
              </button>
              <button
                className={viewMode === 'list' ? 'active' : ''}
                onClick={() => setViewMode('list')}
                title="List view"
                aria-label="List view"
                aria-pressed={viewMode === 'list'}
              >
                <List size={16} />
              </button>
            </div>
          </div>
        </div>
        {montageMode && (
          <div className="montage-bar">
            <div>
              <Layers size={17} />
              <span>
                Pick replays in playback order.{' '}
                <strong>{selectedClips.length} selected.</strong>
              </span>
            </div>
            <div>
              <button
                className="btn-ghost btn-compact"
                onClick={() => setSelectedClips([])}
                disabled={!selectedClips.length}
              >
                Clear
              </button>
              <button
                className="btn-primary btn-compact"
                disabled={selectedClips.length < 2}
                onClick={() => onCreateMontage(selectedClips)}
              >
                Continue with {selectedClips.length} clips
              </button>
            </div>
          </div>
        )}
        <div className="library-scroll" aria-busy={loading}>
          {loading && !clips.length ? (
            <div className="empty-state" role="status">
              <Film size={32} />
              <h2>Finding your replays…</h2>
              <p>Scanning your clips folder.</p>
            </div>
          ) : !clips.length ? (
            <div className="empty-state">
              {filtered ? <Search size={34} /> : <Video size={34} />}
              <h2>
                {filtered
                  ? 'No matching replays.'
                  : 'Your next great play starts here.'}
              </h2>
              <p>
                {filtered ? (
                  'Try another search or clear your filters to see more replays.'
                ) : (
                  <>
                    Press <kbd>{hotkey}</kbd> while playing to save what just
                    happened. You can also add MP4 files to your clips folder.
                  </>
                )}
              </p>
              {filtered ? (
                <button className="btn-secondary" onClick={resetFilters}>
                  Clear filters
                </button>
              ) : (
                <span className="empty-path" title={clipsDir}>
                  {clipsDir}
                </span>
              )}
            </div>
          ) : (
            groups.map((group) => (
              <section
                className="clip-group"
                key={group.label}
                aria-label={group.label}
              >
                <div className="clip-group-header">
                  <h2>{group.label}</h2>
                  <span className="clip-group-count">{group.items.length}</span>
                </div>
                <div
                  className={viewMode === 'grid' ? 'clips-grid' : 'clips-list'}
                >
                  {group.items.map((clip) => {
                    const pickIndex = selectedClips.findIndex(
                      (item) => item.id === clip.id,
                    );
                    const selected = pickIndex !== -1;
                    return (
                      <article
                        key={clip.id}
                        className={`${viewMode === 'grid' ? 'clip-card' : 'clip-row'} ${selected ? 'selected' : ''}`}
                      >
                        <div className="clip-thumb-wrap">
                          {clip.thumb_path ? (
                            <img
                              className="clip-thumb"
                              src={mediaUrl(clip.thumb_path)}
                              alt=""
                              loading="lazy"
                            />
                          ) : (
                            <div className="clip-thumb-fallback">
                              <Film size={30} />
                            </div>
                          )}
                          <button
                            className="clip-open"
                            onClick={() => openClip(clip)}
                            aria-label={
                              montageMode
                                ? `${selected ? 'Deselect' : 'Select'} ${clip.title}`
                                : `Open ${clip.title}`
                            }
                            aria-pressed={montageMode ? selected : undefined}
                          >
                            <span className="thumb-hover-play">
                              <span className="play-circle">
                                {montageMode ? (
                                  selected ? (
                                    <Check size={22} />
                                  ) : (
                                    <Layers size={22} />
                                  )
                                ) : (
                                  <Play size={22} fill="currentColor" />
                                )}
                              </span>
                            </span>
                          </button>
                          <div className="thumb-top-left">
                            {montageMode ? (
                              <span
                                className={`montage-order-badge ${selected ? 'picked' : ''}`}
                              >
                                {selected ? pickIndex + 1 : '+'}
                              </span>
                            ) : (
                              <span className="track-badge">
                                {clip.height > 0 ? `${clip.height}p` : 'Video'}
                                <span>{clip.audio_tracks} audio</span>
                              </span>
                            )}
                          </div>
                          <button
                            className={`fav-btn ${clip.favorite ? 'is-fav' : ''}`}
                            aria-label={`${clip.favorite ? 'Unfavorite' : 'Favorite'} ${clip.title}`}
                            aria-pressed={clip.favorite}
                            onClick={() => onToggleFavorite(clip)}
                          >
                            <Star
                              size={15}
                              fill={clip.favorite ? 'currentColor' : 'none'}
                            />
                          </button>
                          <span className="duration-badge">
                            {formatDuration(clip.duration_ms)}
                          </span>
                        </div>
                        <div className="clip-body">
                          <div className="clip-title-row">
                            <button
                              className="clip-title"
                              onClick={() => openClip(clip)}
                              title={clip.title}
                            >
                              {clip.title}
                            </button>
                            {clip.tag && (
                              <span className="clip-tag">{clip.tag}</span>
                            )}
                          </div>
                          <div className="clip-meta-row">
                            <span>
                              {formatRelativeAge(clip.created_at)} ·{' '}
                              {formatBytes(clip.size_bytes)}
                            </span>
                            {renderActions(clip)}
                          </div>
                        </div>
                      </article>
                    );
                  })}
                </div>
              </section>
            ))
          )}
        </div>
        <footer className="library-footer">
          <span>Keep the play. Cut the rest.</span>
          <span>
            Save a moment <kbd>{hotkey}</kbd>
          </span>
        </footer>
      </div>
    </main>
  );
}
