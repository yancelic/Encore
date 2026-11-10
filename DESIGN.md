---
version: beta
name: encore-skeuomorphic-studio
description: Tactile dark-hardware skeuomorphic interface for Encore game clipping and multi-track replay editing, featuring brushed anodized gunmetal chassis, recessed smoked-glass LCD readouts, domed jewel LED indicators, physical monitor-bezel clip cartridges, and an illuminated multi-track mixing console.

colors:
  chassis-deep: "#111318"
  chassis-base: "#1B1E26"
  chassis-metal: "#252933"
  chassis-raised: "#2F3542"
  chassis-highlight: "#3C4354"
  glass-well: "#0B0D12"
  bezel-dark: "#08090D"
  hairline-groove: "#0E1015"
  hairline-catchlight: "rgba(255, 255, 255, 0.08)"
  ink: "#F1F4F9"
  ink-engraved: "#A0AABF"
  ink-dim: "#6F788C"
  primary: "#4C64F7"
  primary-lit: "#6E83FF"
  primary-deep: "#2E40B8"
  accent-teal: "#16C9B3"
  accent-blue: "#3B82F6"
  accent-amber: "#F59E0B"
  danger: "#EF4444"

typography:
  display-lg:
    fontFamily: "'Segoe UI Variable', 'Segoe UI', 'Inter', system-ui, sans-serif"
    fontSize: 19px
    fontWeight: 700
    lineHeight: 1.25
    letterSpacing: -0.02em
  section-title:
    fontFamily: "'Segoe UI Variable', 'Segoe UI', 'Inter', system-ui, sans-serif"
    fontSize: 13px
    fontWeight: 700
    lineHeight: 1.3
    letterSpacing: 0.03em
  body:
    fontFamily: "'Segoe UI Variable', 'Segoe UI', 'Inter', system-ui, sans-serif"
    fontSize: 13px
    fontWeight: 500
    lineHeight: 1.45
    letterSpacing: 0em
  caption:
    fontFamily: "'Segoe UI Variable', 'Segoe UI', 'Inter', system-ui, sans-serif"
    fontSize: 11px
    fontWeight: 600
    lineHeight: 1.35
    letterSpacing: 0.01em
  mono:
    fontFamily: "'Cascadia Mono', 'JetBrains Mono', Consolas, monospace"
    fontSize: 11px
    fontWeight: 600
    lineHeight: 1.2
    letterSpacing: 0em

spacing:
  page-pad: 20px
  card-gap: 16px
  panel-pad: 16px
  control-gap: 8px
  track-gap: 7px

radius:
  sm: 6px
  md: 8px
  lg: 12px
  pill: 999px

components:
  top-bar:
    height: 62px
    surface: "brushed anodized gunmetal faceplate with top catchlight and recessed seam"
    borderBottom: "1px solid #0B0D12"
  clip-card:
    surface: "machined dark aluminum cartridge shell with recessed 16:9 glass monitor well"
    radius: 12px
    thumbnailAspect: "16 / 9"
  timeline-tracks:
    video: "#16C9B3"
    game-audio: "#16C9B3"
    chat-audio: "#3B82F6"
    mic-audio: "#F59E0B"
  cta-primary:
    background: "linear-gradient(180deg, #6E84FF 0%, #4E64F5 48%, #3447C4 100%)"
    color: "#FFFFFF"
    radius: 8px
  status-pill:
    surface: "recessed smoked-glass LCD readout bay with domed jewel LED"
    readyColor: "#16C9B3"
    recordingColor: "#EF4444"
    savingColor: "#F59E0B"
    offlineColor: "#6F788C"
---

# Encore Hardware Skeuomorphic Design System

## Aesthetic Direction & Physical Lighting Law
Encore is designed like a **precision-machined desktop capture deck and studio broadcast console** — inspired by classic pro audio/video hardware, brushed dark anodized aluminum, smoked-glass OLED/LCD meters, and illuminated console switchgear.

A single consistent **overhead studio key light** governs every surface:
- **Raised tactile elements** (push-buttons, cartridge clip cards, rocker toggles, keycaps, fader caps, trim calipers) have a crisp bright top-edge specular rim (`rgba(255,255,255,0.14–0.28)`), a subtle convex vertical gradient, and a crisp bottom ambient drop shadow. Pressing them physically depresses the element (`translateY(1px)`), compressing its drop shadow.
- **Recessed wells & cavities** (smoked-glass LCD status displays, search inputs, timeline tape troughs, switch bays, video monitor aperture) cast a dark top inner shadow (`inset 0 2px 5px rgba(0,0,0,0.75)`) and catch a faint bright catchlight along their bottom lip (`0 1px 0 rgba(255,255,255,0.07)`).
- **Engraved labels** carry subtle recessed text shadows (`0 -1px 0 rgba(0,0,0,0.7)`) while bright gameplay thumbnails and illuminated jewel LEDs provide crisp focal contrast against the dark machined chassis.

## Layout & Hierarchy
1. **Master Hardware Header (`TopBar`)**:
   - Brushed gunmetal faceplate (`62px`) with recessed lens-emblem brand badge and engraved `Encore` nameplate.
   - Smoked-glass status bay with domed physical LED jewel (`Ready`, pulsing `Recording`, `Saving`, or dim `Offline`), convex cobalt `Start Recorder` / `Save Clip` keycap, recessed `Games | Active Window` rocker switch bay, recessed LCD quality readout (`20s · 1080p · 60fps`), and a sculpted mechanical hotkey keycap (`Alt+S`).
2. **Media Rack Library (`LibraryView`)**:
   - Brushed-metal toolbar with recessed search well, machined dropdown selectors, and recessed `Grid | List` toggle housing.
   - Engraved dual-tone horizontal channel dividers (`Today · VALORANT`).
   - **Monitor-Bezel Clip Cartridges**: Each 16:9 thumbnail sits inside a recessed glass monitor frame surrounded by a chamfered aluminum housing, topped with smoked-glass resolution/track-LED badge, physical embossed favorite star jewel, engraved duration badge, and raised tactile `Copy`, `Folder`, and `Delete` micro-buttons.
3. **Broadcast Clip Studio (`EditorView`)**:
   - **Reference Monitor Stage**: Centered 16:9 monitor bezel housing the playback screen and optional illuminated `9:16` viewfinder crop reticle.
   - **Studio Rack Inspector**: Brushed gunmetal side rack with carved input wells (`Title`, `Tag`, `Notes`), interlocking `Original` / `9:16` canvas switch keys, and tactile export/file actions.
   - **Multi-Track Mixing & Transport Deck**: Chunky machined transport keys around a circular domed Play/Pause console button, recessed smoked-glass master timecode LCD, physical slotted volume fader with knurled silver cap, aligned metallic timeline ruler with time ticks, backlit `MUTE` channel-strip buttons, recessed waveform troughs, and machined emerald trim calipers with knurled grip notches.
4. **Rack-Mounted Modals (`SettingsModal` & `MontageModal`)**:
   - Chamfered gunmetal chassis with precision corner fastener accents, carved compartment wells, sculpted hotkey macro button, and a physical sliding chrome barrel toggle switch for Windows autostart.
