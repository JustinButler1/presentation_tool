//! Compare: import screenshots, link them into named before/after pairs,
//! group those pairs, and scrub a mask across the current pair.
//!
//! Before is always the left side. After is always the right side.
//! Up and down move between pairs in the viewer filter. The pair above and
//! the pair below stay visible, except at the ends.

use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    Animation, AnimationExt, App, Bounds, ClickEvent, Context, CursorStyle, DispatchPhase,
    ExternalPaths, FocusHandle, FontWeight, KeyBinding, KeyDownEvent, Menu, MenuItem, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ObjectFit, Pixels, Point, SharedString,
    TitlebarOptions, UniformListScrollHandle, Window, WindowAppearance, WindowBounds,
    WindowControlArea, WindowOptions, actions, canvas, div, img, point, prelude::*, px, rgb, size,
    svg, uniform_list,
};
use gpui_platform::application;

use crate::model::{
    CompareMode, Library, PairOrder, Pin, Proposal, SPLIT_MAX, SPLIT_MIN, ViewState, discard_stored_copy,
    highlight_spans, pair_position, photo_label, prepare_paths, pretty_caption, propose_numbered,
    ready_pairs, ready_thumb, search_pairs, source_label, step_index, suggest_name, write_asset,
    write_thumbnail,
};

actions!(
    compare,
    [
        Quit,
        AddPhotos,
        NextGroup,
        PrevGroup,
        NudgeLeft,
        NudgeRight,
        SnapLeft,
        SnapRight,
        OpenViewer,
        ToggleMode,
        Unlink,
        Find,
        PasteImage,
    ]
);

const BG: u32 = 0x121212;
const CARD: u32 = 0x1A1A1A;
const LINE: u32 = 0x2C2C2E;
const TEXT: u32 = 0xF5F5F7;
const MUTED: u32 = 0x8E8E93;
const BLUE: u32 = 0x2F6BFF;
const FIELD: u32 = 0x181818;
/// Header surface, a step above the page so the bar stays readable.
const HEADER_BG: u32 = 0x242426;
const HOVER: u32 = 0x3A3A3C;
const RED: u32 = 0xFF453A;

const HEADER: f32 = 52.0;
const FOOTER: f32 = 44.0;
const VIEW_PAD: f32 = 18.0;
const GAP: f32 = 12.0;
const NUDGE: f32 = 0.02;
const NAME_LIMIT: usize = 80;
const NOTE_LIMIT: usize = 500;
const NOTES_W: f32 = 300.0;
const MENU: u32 = 0x242424;
const PAIR_ROW_H: f32 = 104.0;
/// Edit-mode checkbox inset from the left edge of a pair row.
const PAIR_PICK_LEFT: f32 = 16.0;
/// Space between that checkbox and the pair card.
const PAIR_PICK_GAP: f32 = 12.0;
const PAIR_PICK_SIZE: f32 = 26.0;
/// Unpaired checkbox inset, clear of the card's rounded clip.
const PHOTO_PICK_TOP: f32 = 22.0;
const PHOTO_PICK_LEFT: f32 = 18.0;
/// Air between the Unpaired bar and the first photo row.
const UNPAIRED_PAD: f32 = 12.0;
const GRID_GAP: f32 = 16.0;
const LIBRARY_CARD_W: f32 = 236.0;
const LIBRARY_IMAGE_H: f32 = 150.0;
const SELECT_CARD_W: f32 = 176.0;
const SELECT_IMAGE_H: f32 = 112.0;
/// Horizontal padding on both sides of a photo row.
const LIBRARY_CHROME: f32 = 56.0;
const SIDEBAR_W: f32 = 220.0;

const DROP_ICON: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="72" height="64" viewBox="0 0 72 64" fill="none">
  <rect x="20" y="6" width="44" height="36" rx="7" stroke="#6E6E73" stroke-width="1.6"/>
  <rect x="8" y="18" width="44" height="36" rx="7" fill="#121212" stroke="#D1D1D6" stroke-width="1.6"/>
  <circle cx="20" cy="30" r="3" stroke="#D1D1D6" stroke-width="1.5"/>
  <path d="M14 48l10-12 8 8 6-8 10 12" stroke="#D1D1D6" stroke-width="1.5" fill="none" stroke-linejoin="round" stroke-linecap="round"/>
</svg>"##;

/// Single-tint icons. GPUI paints `svg` as an alpha mask, so the stroke is
/// opaque and the element color supplies the tint.
const STACK_ICON: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16" fill="none">
  <rect x="4.2" y="1.6" width="9.2" height="9.2" rx="1.6" stroke="#fff" stroke-width="1.4"/>
  <rect x="2.2" y="4.6" width="9.2" height="9.2" rx="1.6" fill="#000" stroke="#fff" stroke-width="1.4"/>
</svg>"##;

const FOLDER_ICON: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16" fill="none">
  <path d="M1.8 4.4c0-.9.7-1.6 1.6-1.6h2.3l1.2 1.4h5.7c.9 0 1.6.7 1.6 1.6v6.2c0 .9-.7 1.6-1.6 1.6H3.4c-.9 0-1.6-.7-1.6-1.6V4.4z" stroke="#fff" stroke-width="1.3" stroke-linejoin="round"/>
</svg>"##;

const SEARCH_ICON: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 20 20" fill="none">
  <circle cx="8.6" cy="8.6" r="5.1" stroke="#fff" stroke-width="1.8"/>
  <path d="M12.5 12.5 L17.2 17.2" stroke="#fff" stroke-width="1.8" stroke-linecap="round"/>
</svg>"##;

const CHEVRON_LEFT: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 20 20" fill="none">
  <path d="M12.2 4.4 L6.6 10 L12.2 15.6" stroke="#fff" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/>
</svg>"##;

const CHEVRON_RIGHT: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 20 20" fill="none">
  <path d="M7.8 4.4 L13.4 10 L7.8 15.6" stroke="#fff" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/>
</svg>"##;

const CHEVRON_UP: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 20 20" fill="none">
  <path d="M4.6 12.4 L10 7 L15.4 12.4" stroke="#fff" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/>
</svg>"##;

const CHEVRON_DOWN: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 20 20" fill="none">
  <path d="M4.6 7.6 L10 13 L15.4 7.6" stroke="#fff" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/>
</svg>"##;

const PLUS_ICON: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 20 20" fill="none">
  <path d="M10 4.2 V15.8 M4.2 10 H15.8" stroke="#fff" stroke-width="1.8" stroke-linecap="round"/>
</svg>"##;

const SCREEN_ICON: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 20 20" fill="none">
  <rect x="2.4" y="3.2" width="15.2" height="10.2" rx="1.6" stroke="#fff" stroke-width="1.6"/>
  <path d="M7.2 16.6 H12.8" stroke="#fff" stroke-width="1.6" stroke-linecap="round"/>
  <path d="M10 13.4 V16.6" stroke="#fff" stroke-width="1.6" stroke-linecap="round"/>
</svg>"##;

const SORT_ICON: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 20 20" fill="none">
  <path d="M6.2 3.6 V16.4" stroke="#fff" stroke-width="1.6" stroke-linecap="round"/>
  <path d="M3.5 6.4 L6.2 3.6 L8.9 6.4" stroke="#fff" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>
  <path d="M13.8 16.4 V3.6" stroke="#fff" stroke-width="1.6" stroke-linecap="round"/>
  <path d="M11.1 13.6 L13.8 16.4 L16.5 13.6" stroke="#fff" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>
</svg>"##;

const LINK_ICON: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 20 20" fill="none">
  <circle cx="5.2" cy="10" r="2.4" stroke="#fff" stroke-width="1.6"/>
  <circle cx="14.8" cy="10" r="2.4" stroke="#fff" stroke-width="1.6"/>
  <path d="M7.6 10 H12.4" stroke="#fff" stroke-width="1.6" stroke-linecap="round"/>
</svg>"##;

const PAIR_ICON: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 20 20" fill="none">
  <rect x="2.2" y="3.6" width="6.4" height="12.8" rx="1.4" stroke="#fff" stroke-width="1.6"/>
  <rect x="11.4" y="3.6" width="6.4" height="12.8" rx="1.4" stroke="#fff" stroke-width="1.6"/>
</svg>"##;

const PENCIL_ICON: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 20 20" fill="none">
  <path d="M12.2 3.2 L16.8 7.8 L7.4 17.2 H2.8 V12.6 Z" stroke="#fff" stroke-width="1.5" stroke-linejoin="round"/>
  <path d="M10.2 5.2 L14.8 9.8" stroke="#fff" stroke-width="1.5" stroke-linecap="round"/>
</svg>"##;

const TRASH_ICON: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 20 20" fill="none">
  <path d="M4 6.2 H16" stroke="#fff" stroke-width="1.6" stroke-linecap="round"/>
  <path d="M7.6 6.2 V4.4 H12.4 V6.2" stroke="#fff" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>
  <path d="M5.3 6.2 L6.2 16.2 H13.8 L14.7 6.2" stroke="#fff" stroke-width="1.6" stroke-linejoin="round"/>
  <path d="M8.4 9 V13.4 M11.6 9 V13.4" stroke="#fff" stroke-width="1.6" stroke-linecap="round"/>
</svg>"##;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Library,
    Create,
    NewGroup,
    Viewer,
    Edit,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FitMode {
    Fit,
    Fill,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    PairName,
    GroupName,
    BeforePattern,
    AfterPattern,
    Note(u64),
    PinTitle(u64),
    PinBody(u64),
    Search,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SlotSide {
    Before,
    After,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SearchScope {
    Current,
    All,
}

#[derive(Clone)]
enum SlotValue {
    Photo(u64),
    Paste(PathBuf),
}

enum PasteUndo {
    Create {
        side: SlotSide,
        previous: Option<PathBuf>,
        current: PathBuf,
    },
    Edit {
        side: SlotSide,
        previous: Option<SlotValue>,
        current: PathBuf,
    },
}

struct Toast {
    message: String,
    undo: bool,
}

#[derive(Clone)]
enum PinGesture {
    Draw { id: u64, ax: f32, ay: f32 },
    Move { id: u64, dx: f32, dy: f32 },
    Resize { id: u64 },
}

struct RowDrag {
    pair_id: u64,
    from: usize,
    target: usize,
    cursor: Point<Pixels>,
    origin: Point<Pixels>,
    grab_x: f32,
    grab_y: f32,
    width: f32,
    moved: bool,
}

struct SearchHit {
    slot: usize,
    title: String,
    before_name: String,
    after_name: String,
    groups: String,
    before: PathBuf,
    after: PathBuf,
    before_id: u64,
    after_id: u64,
}

#[derive(Clone)]
struct PairRow {
    index: usize,
    position: usize,
    pair_id: u64,
    name: String,
    label: String,
    before: PathBuf,
    after: PathBuf,
    before_id: u64,
    after_id: u64,
}

pub struct Studio {
    focus: FocusHandle,
    library: Library,
    screen: Screen,
    selected_pair: usize,
    focus_pair: usize,
    dragging: bool,
    frame: Rc<Cell<Option<Bounds<Pixels>>>>,
    pair_name: String,
    name_edited: bool,
    focused: Option<Field>,
    caret: usize,
    fit: FitMode,
    fit_menu: bool,
    /// `None` is the All pairs list. A group id shows only that group's pairs.
    library_group: Option<u64>,
    /// Empty means every pair. Otherwise the unique union of these groups.
    viewer_groups: Vec<u64>,
    groups_menu: bool,
    group_picks: Vec<u64>,
    compare_mode: CompareMode,
    blend: f32,
    blend_drag: bool,
    blend_frame: Rc<Cell<Option<Bounds<Pixels>>>>,
    mode_menu: bool,
    order_menu: bool,
    /// Last painted bounds of the sort pill, in window coordinates.
    order_anchor: Rc<Cell<Option<Bounds<Pixels>>>>,
    notes_open: bool,
    placing: bool,
    pin_drag: Option<PinGesture>,
    selected_pin: Option<u64>,
    auto_pair: bool,
    before_pattern: String,
    after_pattern: String,
    search_open: bool,
    search_query: String,
    search_scope: SearchScope,
    search_selected: usize,
    scope_menu: bool,
    library_scroll: UniformListScrollHandle,
    row_bounds: Rc<RefCell<Vec<Bounds<Pixels>>>>,
    row_drag: Option<RowDrag>,
    slot: SlotSide,
    paste_before: Option<PathBuf>,
    paste_after: Option<PathBuf>,
    editing: Option<u64>,
    edit_before: Option<SlotValue>,
    edit_after: Option<SlotValue>,
    edit_return: Screen,
    paste_undo: Option<PasteUndo>,
    toast: Option<Toast>,
    staged: Vec<PathBuf>,
    /// Swallow the click that follows a completed row drag.
    block_row_open: bool,
    /// The unpaired photo grid under the pair list. Closed hides the grid.
    unpaired_open: bool,
    /// Pencil toggle. Shows a trash control on each pair and each loose photo.
    arrange: bool,
    /// Pairs marked while arrange mode is on.
    picked_pairs: Vec<u64>,
    /// Loose photos marked while arrange mode is on.
    picked_photos: Vec<u64>,
    /// Confirmation before removing every marked pair and loose photo.
    confirm_delete: bool,
    /// True while a background pass is writing thumbnails for photos already on disk.
    thumb_job: bool,
}

impl Studio {
    fn new(focus: FocusHandle) -> Self {
        let stored = Library::load_app();
        let (library, compare_mode, blend, fit, view) = match stored {
            Some(stored) => (
                stored.library,
                stored.compare_mode,
                stored.blend,
                if stored.fit_fill { FitMode::Fill } else { FitMode::Fit },
                stored.view,
            ),
            None => (
                Library::new(),
                CompareMode::Wipe,
                0.5,
                FitMode::Fit,
                ViewState::default(),
            ),
        };
        let focus_pair = view
            .focus_pair
            .and_then(|id| pair_position(&library, id))
            .unwrap_or(0);
        let library_group = view
            .library_group
            .filter(|id| library.group(*id).is_some());
        let viewer_groups = view
            .viewer_groups
            .into_iter()
            .filter(|id| library.group(*id).is_some())
            .collect();
        Self {
            focus,
            library,
            screen: Screen::Library,
            selected_pair: focus_pair,
            focus_pair,
            dragging: false,
            frame: Rc::new(Cell::new(None)),
            pair_name: String::new(),
            name_edited: false,
            focused: None,
            caret: 0,
            fit,
            fit_menu: false,
            library_group,
            viewer_groups,
            groups_menu: false,
            group_picks: Vec::new(),
            compare_mode,
            blend,
            blend_drag: false,
            blend_frame: Rc::new(Cell::new(None)),
            mode_menu: false,
            order_menu: false,
            order_anchor: Rc::new(Cell::new(None)),
            notes_open: false,
            placing: false,
            pin_drag: None,
            selected_pin: None,
            auto_pair: false,
            before_pattern: "before_{n}".to_string(),
            after_pattern: "after_{n}".to_string(),
            search_open: false,
            search_query: String::new(),
            search_scope: SearchScope::All,
            search_selected: 0,
            scope_menu: false,
            library_scroll: UniformListScrollHandle::new(),
            row_bounds: Rc::new(RefCell::new(Vec::new())),
            row_drag: None,
            slot: SlotSide::Before,
            paste_before: None,
            paste_after: None,
            editing: None,
            edit_before: None,
            edit_after: None,
            edit_return: Screen::Library,
            paste_undo: None,
            toast: None,
            staged: Vec::new(),
            block_row_open: false,
            unpaired_open: true,
            arrange: false,
            picked_pairs: Vec::new(),
            picked_photos: Vec::new(),
            confirm_delete: false,
            thumb_job: false,
        }
    }

    /// Fill in thumbnails for photos imported before thumbnails existed.
    /// New imports write theirs before the library repaints.
    fn warm_thumbs(&mut self, cx: &mut Context<Self>) {
        if self.thumb_job {
            return;
        }
        let missing: Vec<PathBuf> = self
            .library
            .photos()
            .iter()
            .filter(|photo| ready_thumb(&photo.path).is_none())
            .map(|photo| photo.path.clone())
            .collect();
        if missing.is_empty() {
            return;
        }
        self.thumb_job = true;
        cx.spawn(async move |this, cx| {
            let mut start = 0;
            while start < missing.len() {
                let end = (start + 6).min(missing.len());
                let batch = missing[start..end].to_vec();
                start = end;
                cx.background_spawn(async move {
                    for path in batch {
                        let _ = write_thumbnail(&path);
                    }
                })
                .await;
                let _ = this.update(cx, |_, cx| cx.notify());
            }
            let _ = this.update(cx, |this, cx| {
                this.thumb_job = false;
                cx.notify();
            });
        })
        .detach();
    }

    fn view_state(&self) -> ViewState {
        ViewState {
            focus_pair: self.library.pairs.get(self.focus_pair).map(|pair| pair.id),
            library_group: self
                .library_group
                .filter(|id| self.library.group(*id).is_some()),
            viewer_groups: self
                .viewer_groups
                .iter()
                .copied()
                .filter(|id| self.library.group(*id).is_some())
                .collect(),
        }
    }

    fn persist(&self) {
        self.library.save_app(
            self.compare_mode,
            self.blend,
            self.fit == FitMode::Fill,
            &self.view_state(),
        );
    }

    fn text_focused(&self) -> bool {
        self.focused.is_some()
    }

    fn visible_indices(&self) -> Vec<usize> {
        self.library.visible_pairs(&self.viewer_groups)
    }

    fn listed_pairs(&self) -> Vec<usize> {
        match self.library_group {
            Some(id) if self.library.group(id).is_some() => self.library.visible_pairs(&[id]),
            _ => (0..self.library.pairs.len()).collect(),
        }
    }

    fn refocus_visible(&mut self) {
        let visible = self.visible_indices();
        if visible.is_empty() {
            return;
        }
        if !visible.contains(&self.focus_pair) {
            self.focus_pair = visible[0];
            self.selected_pair = self.focus_pair;
        }
    }

    fn pick_files(&mut self, cx: &mut Context<Self>) {
        let dialog = rfd::AsyncFileDialog::new()
            .set_title("Add images")
            .add_filter(
                "Images",
                &["png", "jpg", "jpeg", "webp", "gif", "bmp", "tif", "tiff", "heic", "heif"],
            )
            .pick_files();
        cx.spawn(async move |this, cx| {
            let Some(files) = dialog.await else {
                return;
            };
            let paths = files
                .iter()
                .map(|file| file.path().to_path_buf())
                .collect();
            Self::finish_import(this, cx, paths).await;
        })
        .detach();
    }

    fn ingest(&mut self, paths: Vec<std::path::PathBuf>, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            Self::finish_import(this, cx, paths).await;
        })
        .detach();
    }

    async fn finish_import(
        this: gpui::WeakEntity<Self>,
        cx: &mut gpui::AsyncApp,
        paths: Vec<std::path::PathBuf>,
    ) {
        let prepared = cx
            .background_spawn(async move { prepare_paths(paths) })
            .await;
        let _ = this.update(cx, |this, cx| {
            if this.library.add_prepared(prepared) > 0 {
                if this.screen == Screen::Viewer {
                    this.screen = Screen::Library;
                    this.dragging = false;
                }
                this.persist();
            }
            cx.notify();
        });
    }

    fn add_photos(&mut self, _: &AddPhotos, _: &mut Window, cx: &mut Context<Self>) {
        self.pick_files(cx);
    }

    fn next_group(&mut self, _: &NextGroup, _: &mut Window, cx: &mut Context<Self>) {
        self.step(1, cx);
    }

    fn prev_group(&mut self, _: &PrevGroup, _: &mut Window, cx: &mut Context<Self>) {
        self.step(-1, cx);
    }

    fn step(&mut self, delta: i32, cx: &mut Context<Self>) {
        if self.search_open {
            self.move_search(delta, cx);
            return;
        }
        self.step_pairs(delta, cx);
    }

    fn step_pairs(&mut self, delta: i32, cx: &mut Context<Self>) {
        if self.screen != Screen::Viewer
            || self.text_focused()
            || self.auto_pair
            || self.search_open
        {
            cx.propagate();
            return;
        }
        self.close_menus();
        let visible = self.visible_indices();
        let position = visible
            .iter()
            .position(|index| *index == self.focus_pair)
            .unwrap_or(0);
        let next = step_index(position, visible.len(), delta);
        if let Some(&index) = visible.get(next) {
            self.focus_pair = index;
            self.selected_pair = index;
            self.selected_pin = None;
        }
        cx.notify();
    }

    fn close_menus(&mut self) {
        self.fit_menu = false;
        self.groups_menu = false;
        self.mode_menu = false;
        self.order_menu = false;
        self.scope_menu = false;
    }

    fn nudge_left(&mut self, _: &NudgeLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.nudge(-NUDGE, cx);
    }

    fn nudge_right(&mut self, _: &NudgeRight, _: &mut Window, cx: &mut Context<Self>) {
        self.nudge(NUDGE, cx);
    }

    fn snap_left(&mut self, _: &SnapLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.snap(SPLIT_MIN, cx);
    }

    fn snap_right(&mut self, _: &SnapRight, _: &mut Window, cx: &mut Context<Self>) {
        self.snap(SPLIT_MAX, cx);
    }

    fn nudge(&mut self, delta: f32, cx: &mut Context<Self>) {
        if self.blocks_mask() {
            cx.propagate();
            return;
        }
        if self.library.nudge_split(self.focus_pair, delta) {
            self.persist();
            cx.notify();
        }
    }

    fn snap(&mut self, split: f32, cx: &mut Context<Self>) {
        if self.blocks_mask() {
            cx.propagate();
            return;
        }
        if self.library.set_split(self.focus_pair, split) {
            self.persist();
            cx.notify();
        }
    }

    fn blocks_mask(&self) -> bool {
        self.text_focused()
            || self.search_open
            || self.auto_pair
            || self.placing
            || self.screen != Screen::Viewer
            || self.compare_mode != CompareMode::Wipe
    }

    fn open_viewer(&mut self, _: &OpenViewer, _: &mut Window, cx: &mut Context<Self>) {
        if self.confirm_delete {
            self.commit_delete_picked(cx);
            return;
        }
        if matches!(self.focused, Some(Field::Note(_) | Field::PinBody(_))) {
            cx.propagate();
            return;
        }
        match enter_action(self.search_open, self.auto_pair, self.screen) {
            EnterAction::OpenSearch => self.open_search_result(cx),
            EnterAction::CreateAutoPairs => self.commit_auto_pair(cx),
            EnterAction::SaveCreate => self.commit_pair(cx),
            EnterAction::SaveEdit => self.save_edit(cx),
            EnterAction::CreateGroup => self.commit_group(cx),
            EnterAction::EnterViewer => self.enter_viewer(cx),
        }
    }

    fn enter_viewer(&mut self, cx: &mut Context<Self>) {
        self.viewer_groups = match self.library_group {
            Some(id) if self.library.group(id).is_some() => vec![id],
            _ => Vec::new(),
        };
        let visible = self.visible_indices();
        if visible.is_empty() {
            return;
        }
        if !visible.contains(&self.selected_pair) {
            self.selected_pair = visible[0];
        }
        self.focus_pair = self.selected_pair;
        self.screen = Screen::Viewer;
        self.dragging = false;
        self.focused = None;
        self.close_menus();
        self.search_open = false;
        self.auto_pair = false;
        cx.notify();
    }

    fn toggle_mode(&mut self, _: &ToggleMode, _: &mut Window, cx: &mut Context<Self>) {
        match escape_action(&EscapeState {
            placing: self.placing || self.pin_drag.is_some(),
            row_drag: self.row_drag.is_some(),
            search: self.search_open,
            auto_pair: self.auto_pair,
            menu: self.fit_menu
                || self.groups_menu
                || self.mode_menu
                || self.order_menu
                || self.scope_menu,
            focused: self.text_focused(),
            notes: self.notes_open && self.screen == Screen::Viewer,
            arrange: self.arrange,
            confirm: self.confirm_delete,
            screen: self.screen,
        }) {
            EscapeAction::CancelPlace => self.cancel_placement(cx),
            EscapeAction::CancelDrag => {
                self.row_drag = None;
                cx.notify();
            }
            EscapeAction::CloseSearch => self.close_search(cx),
            EscapeAction::CloseAutoPair => {
                self.auto_pair = false;
                self.focused = None;
                cx.notify();
            }
            EscapeAction::CloseMenu => {
                self.close_menus();
                cx.notify();
            }
            EscapeAction::Blur => {
                self.focused = None;
                cx.notify();
            }
            EscapeAction::CloseNotes => {
                self.notes_open = false;
                self.placing = false;
                cx.notify();
            }
            EscapeAction::CloseConfirm => {
                self.confirm_delete = false;
                cx.notify();
            }
            EscapeAction::CloseArrange => {
                self.arrange = false;
                self.confirm_delete = false;
                self.picked_pairs.clear();
                self.picked_photos.clear();
                cx.notify();
            }
            EscapeAction::LeaveViewer => self.show_library(cx),
            EscapeAction::CancelCreate => self.cancel_create(cx),
            EscapeAction::CancelEdit => self.cancel_edit(cx),
            EscapeAction::CancelGroup => self.cancel_group(cx),
            EscapeAction::EnterViewer => self.enter_viewer(cx),
        }
    }

    fn show_library(&mut self, cx: &mut Context<Self>) {
        self.selected_pair = self.focus_pair.min(self.library.pairs.len().saturating_sub(1));
        self.screen = Screen::Library;
        self.dragging = false;
        self.focused = None;
        self.placing = false;
        self.pin_drag = None;
        self.search_open = false;
        self.close_menus();
        cx.notify();
    }

    fn unlink(&mut self, _: &Unlink, _: &mut Window, cx: &mut Context<Self>) {
        if self.confirm_delete {
            return;
        }
        if self.text_focused() || self.search_open || self.auto_pair {
            cx.propagate();
            return;
        }
        if self.screen != Screen::Library || self.library.pairs.is_empty() {
            cx.propagate();
            return;
        }
        let listed = self.listed_pairs();
        let index = if listed.contains(&self.selected_pair) {
            self.selected_pair
        } else {
            listed.first().copied().unwrap_or(0)
        };
        self.library.unlink(index);
        self.clamp_after_unlink(index);
        self.persist();
        cx.notify();
    }

    fn clamp_after_unlink(&mut self, deleted: usize) {
        let len = self.library.pairs.len();
        if len == 0 {
            self.screen = Screen::Library;
            self.selected_pair = 0;
            self.focus_pair = 0;
            return;
        }
        if self.selected_pair > deleted {
            self.selected_pair -= 1;
        }
        if self.focus_pair > deleted {
            self.focus_pair -= 1;
        }
        self.selected_pair = self.selected_pair.min(len - 1);
        self.focus_pair = self.focus_pair.min(len - 1);
    }

    fn remove_listed_pair(&mut self, index: usize, cx: &mut Context<Self>) {
        let pair_id = self.library.pairs.get(index).map(|pair| pair.id);
        let Some(photos) = self.library.take_pair(index) else {
            return;
        };
        if let Some(id) = pair_id {
            self.picked_pairs.retain(|item| *item != id);
        }
        for photo in &photos {
            let id = photo.id;
            self.picked_photos.retain(|item| *item != id);
            self.reclaim_copy(&photo.path);
        }
        self.clamp_after_unlink(index);
        self.persist();
        cx.notify();
    }

    fn remove_loose_photo(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(photo) = self.library.take_loose(id) else {
            return;
        };
        self.picked_photos.retain(|item| *item != id);
        self.reclaim_copy(&photo.path);
        self.persist();
        cx.notify();
    }

    fn toggle_picked_pair(&mut self, id: u64, cx: &mut Context<Self>) {
        toggle_id(&mut self.picked_pairs, id);
        cx.notify();
    }

    fn toggle_picked_photo(&mut self, id: u64, cx: &mut Context<Self>) {
        toggle_id(&mut self.picked_photos, id);
        cx.notify();
    }

    /// Pairs in the current list, then loose photos. Select all uses this set.
    fn visible_pick_targets(&self) -> (Vec<u64>, Vec<u64>) {
        let pairs = self
            .listed_pairs()
            .into_iter()
            .filter_map(|index| self.library.pairs.get(index).map(|pair| pair.id))
            .collect();
        let photos = self
            .library
            .unassigned()
            .into_iter()
            .map(|photo| photo.id)
            .collect();
        (pairs, photos)
    }

    fn toggle_select_all(&mut self, cx: &mut Context<Self>) {
        let (pairs, photos) = self.visible_pick_targets();
        if selection_covers(&pairs, &photos, &self.picked_pairs, &self.picked_photos) {
            self.picked_pairs.clear();
            self.picked_photos.clear();
        } else {
            self.picked_pairs = pairs;
            self.picked_photos = photos;
        }
        cx.notify();
    }

    fn ask_delete_picked(&mut self, cx: &mut Context<Self>) {
        if self.picked_pairs.is_empty() && self.picked_photos.is_empty() {
            return;
        }
        self.confirm_delete = true;
        cx.notify();
    }

    fn cancel_delete(&mut self, cx: &mut Context<Self>) {
        self.confirm_delete = false;
        cx.notify();
    }

    fn commit_delete_picked(&mut self, cx: &mut Context<Self>) {
        self.confirm_delete = false;
        let focus_id = self.library.pairs.get(self.focus_pair).map(|pair| pair.id);
        let selected_id = self
            .library
            .pairs
            .get(self.selected_pair)
            .map(|pair| pair.id);
        let photo_ids = self.picked_photos.clone();
        let mut indices: Vec<usize> = self
            .picked_pairs
            .iter()
            .filter_map(|id| self.library.pairs.iter().position(|pair| pair.id == *id))
            .collect();
        indices.sort_unstable();
        indices.reverse();
        let mut changed = false;
        for index in indices {
            if let Some(photos) = self.library.take_pair(index) {
                changed = true;
                for photo in &photos {
                    self.reclaim_copy(&photo.path);
                }
            }
        }
        for id in photo_ids {
            if let Some(photo) = self.library.take_loose(id) {
                changed = true;
                self.reclaim_copy(&photo.path);
            }
        }
        self.picked_pairs.clear();
        self.picked_photos.clear();
        self.restore_pair_focus(focus_id, selected_id);
        if changed {
            self.persist();
        }
        cx.notify();
    }

    fn restore_pair_focus(&mut self, focus_id: Option<u64>, selected_id: Option<u64>) {
        let len = self.library.pairs.len();
        if len == 0 {
            self.screen = Screen::Library;
            self.selected_pair = 0;
            self.focus_pair = 0;
            return;
        }
        self.focus_pair = focus_id
            .and_then(|id| self.library.pairs.iter().position(|pair| pair.id == id))
            .unwrap_or_else(|| self.focus_pair.min(len - 1));
        self.selected_pair = selected_id
            .and_then(|id| self.library.pairs.iter().position(|pair| pair.id == id))
            .unwrap_or(self.focus_pair);
    }

    /// Drop the app's stored copy when nothing in the library still uses it.
    /// The file the user originally imported is outside that store and stays put.
    fn reclaim_copy(&mut self, path: &Path) {
        if self.paste_in_use(path) {
            return;
        }
        discard_stored_copy(path);
    }

    fn open_create(&mut self, cx: &mut Context<Self>) {
        if self.library.unassigned().len() < 2 {
            return;
        }
        self.library.clear_draft();
        self.reset_editor_text();
        self.clear_paste_state();
        self.close_menus();
        self.slot = SlotSide::Before;
        self.screen = Screen::Create;
        cx.notify();
    }

    fn open_new_group(&mut self, cx: &mut Context<Self>) {
        if self.library.pairs.is_empty() {
            return;
        }
        self.pair_name.clear();
        self.name_edited = false;
        self.focused = Some(Field::GroupName);
        self.caret = 0;
        let pair_ids: Vec<u64> = self.library.pairs.iter().map(|pair| pair.id).collect();
        self.group_picks = seeded_group_picks(&pair_ids, &self.picked_pairs);
        self.close_menus();
        self.auto_pair = false;
        self.screen = Screen::NewGroup;
        cx.notify();
    }

    fn cancel_group(&mut self, cx: &mut Context<Self>) {
        self.pair_name.clear();
        self.name_edited = false;
        self.focused = None;
        self.group_picks.clear();
        self.screen = Screen::Library;
        cx.notify();
    }

    fn toggle_group_pick(&mut self, pair_id: u64, cx: &mut Context<Self>) {
        if let Some(index) = self.group_picks.iter().position(|id| *id == pair_id) {
            self.group_picks.remove(index);
        } else {
            self.group_picks.push(pair_id);
        }
        cx.notify();
    }

    fn toggle_group_select_all(&mut self, cx: &mut Context<Self>) {
        let pair_ids: Vec<u64> = self.library.pairs.iter().map(|pair| pair.id).collect();
        self.group_picks = next_group_picks(&pair_ids, &self.group_picks);
        cx.notify();
    }

    fn commit_group(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.library.create_group(&self.pair_name, &self.group_picks) else {
            return;
        };
        self.pair_name.clear();
        self.name_edited = false;
        self.focused = None;
        self.group_picks.clear();
        self.library_group = Some(id);
        let listed = self.listed_pairs();
        if let Some(&first) = listed.first() {
            self.selected_pair = first;
            self.focus_pair = first;
        }
        self.screen = Screen::Library;
        self.persist();
        cx.notify();
    }

    fn select_library_group(&mut self, group: Option<u64>, cx: &mut Context<Self>) {
        self.library_group = group.filter(|id| self.library.group(*id).is_some());
        let listed = self.listed_pairs();
        if let Some(&first) = listed.first()
            && !listed.contains(&self.selected_pair)
        {
            self.selected_pair = first;
            self.focus_pair = first;
        }
        self.persist();
        cx.notify();
    }

    fn toggle_viewer_group(&mut self, group_id: u64, cx: &mut Context<Self>) {
        if let Some(index) = self.viewer_groups.iter().position(|id| *id == group_id) {
            self.viewer_groups.remove(index);
        } else {
            self.viewer_groups.push(group_id);
        }
        self.refocus_visible();
        self.persist();
        cx.notify();
    }

    fn show_all_in_viewer(&mut self, cx: &mut Context<Self>) {
        self.viewer_groups.clear();
        self.groups_menu = false;
        self.refocus_visible();
        self.persist();
        cx.notify();
    }

    fn cancel_create(&mut self, cx: &mut Context<Self>) {
        self.library.clear_draft();
        self.reset_editor_text();
        self.paste_before = None;
        self.paste_after = None;
        self.clear_paste_state();
        self.screen = Screen::Library;
        cx.notify();
    }

    fn commit_pair(&mut self, cx: &mut Context<Self>) {
        if self.create_slot(SlotSide::Before).is_none() || self.create_slot(SlotSide::After).is_none() {
            return;
        }
        if let Some(path) = self.paste_before.clone() {
            let Some(id) = self.adopt_loose(path) else {
                self.fail_paste("That image is already used in another pair.");
                cx.notify();
                return;
            };
            self.library.draft_before = Some(id);
        }
        if let Some(path) = self.paste_after.clone() {
            let Some(id) = self.adopt_loose(path) else {
                self.fail_paste("That image is already used in another pair.");
                cx.notify();
                return;
            };
            self.library.draft_after = Some(id);
        }
        let Some(index) = self.library.commit_draft(&self.pair_name) else {
            self.fail_paste("Choose a before image and an after image.");
            cx.notify();
            return;
        };
        self.reset_editor_text();
        self.paste_before = None;
        self.paste_after = None;
        self.staged.clear();
        self.clear_paste_state();
        self.selected_pair = index;
        self.focus_pair = index;
        self.screen = Screen::Library;
        self.dragging = false;
        self.persist();
        cx.notify();
    }

    fn choose_photo(&mut self, id: u64, from_library: bool) {
        if from_library {
            self.library.clear_draft();
            self.pair_name.clear();
            self.name_edited = false;
            self.paste_before = None;
            self.paste_after = None;
            self.slot = SlotSide::Before;
            self.screen = Screen::Create;
        }
        let before_was = self.library.draft_before;
        let after_was = self.library.draft_after;
        self.library.select_for_draft(id);
        if self.library.draft_before != before_was {
            self.paste_before = None;
        }
        if self.library.draft_after != after_was {
            self.paste_after = None;
        }
        if self.library.draft_before == Some(id) {
            self.slot = SlotSide::Before;
        } else if self.library.draft_after == Some(id) {
            self.slot = SlotSide::After;
        }
        if self.focused == Some(Field::PairName) {
            self.focused = None;
        }
        self.sync_suggested_name();
    }

    fn sync_suggested_name(&mut self) {
        if self.name_edited {
            return;
        }
        self.pair_name = self
            .library
            .draft_before
            .and_then(|id| self.library.photo(id))
            .map(|photo| suggest_name(std::path::Path::new(&photo_label(photo))))
            .unwrap_or_default();
    }

    fn reset_editor_text(&mut self) {
        self.pair_name.clear();
        self.name_edited = false;
        self.focused = None;
        self.caret = 0;
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(field) = self.focused else {
            return;
        };
        let keystroke = &event.keystroke;
        let command = keystroke.modifiers.platform || keystroke.modifiers.control;
        match typing_effect(Some(field), keystroke.key.as_str(), command) {
            Typing::Pass => return,
            Typing::Stop => cx.stop_propagation(),
            Typing::PasteText => {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    self.insert_at_caret(&text);
                    cx.notify();
                }
                cx.stop_propagation();
            }
            Typing::Caret(delta) => {
                self.move_caret(delta);
                cx.notify();
                cx.stop_propagation();
            }
            Typing::Backspace => {
                self.delete_caret(-1);
                cx.notify();
                cx.stop_propagation();
            }
            Typing::Delete => {
                self.delete_caret(1);
                cx.notify();
                cx.stop_propagation();
            }
            Typing::Newline => {
                self.insert_at_caret("\n");
                cx.notify();
                cx.stop_propagation();
            }
            Typing::Insert => {
                if let Some(ch) = keystroke.key_char.as_deref()
                    && ch.chars().count() == 1
                    && !ch.chars().next().unwrap().is_control()
                {
                    self.insert_at_caret(ch);
                    cx.notify();
                }
                cx.stop_propagation();
            }
        }
    }

    fn apply_split(&mut self, position: Point<Pixels>) -> bool {
        let Some(bounds) = self.frame.get() else {
            return false;
        };
        let width = px_f(bounds.size.width);
        if width < 1.0 {
            return false;
        }
        let x = (px_f(position.x) - px_f(bounds.origin.x)) / width;
        self.library.set_split(self.focus_pair, x)
    }

    fn begin_drag(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        if self.placing || self.pin_drag.is_some() {
            return;
        }
        self.dragging = true;
        self.close_menus();
        self.apply_split(position);
        cx.notify();
    }

    fn track_move(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let mut changed = false;
        if self.row_drag.is_some() {
            changed |= self.update_row_target(position);
        }
        if self.dragging {
            changed |= self.apply_split(position);
        }
        if self.pin_drag.is_some() {
            changed |= self.update_pin_drag(position);
        }
        if self.blend_drag {
            changed |= self.apply_blend(position);
        }
        if changed {
            cx.notify();
        }
    }

    fn track_up(&mut self, cx: &mut Context<Self>) {
        let mut changed = false;
        if self.row_drag.is_some() {
            self.finish_row_drag();
            changed = true;
        }
        if self.dragging {
            self.dragging = false;
            self.persist();
            changed = true;
        }
        if self.pin_drag.is_some() {
            self.finish_pin_drag();
            changed = true;
        }
        if self.blend_drag {
            self.blend_drag = false;
            self.persist();
            changed = true;
        }
        if changed {
            cx.notify();
        }
    }

    fn focus_field(&mut self, field: Field) {
        self.focused = Some(field);
        self.caret = self.field_text().chars().count();
    }

    fn field_text(&self) -> String {
        match self.focused {
            Some(Field::PairName | Field::GroupName) => self.pair_name.clone(),
            Some(Field::BeforePattern) => self.before_pattern.clone(),
            Some(Field::AfterPattern) => self.after_pattern.clone(),
            Some(Field::Search) => self.search_query.clone(),
            Some(Field::Note(id)) => self.note_text(id),
            Some(Field::PinTitle(id)) => self.pin_parts(id).0,
            Some(Field::PinBody(id)) => self.pin_parts(id).1,
            None => String::new(),
        }
    }

    fn set_field_text(&mut self, text: String) {
        match self.focused {
            Some(Field::PairName | Field::GroupName) => {
                self.pair_name = text;
                self.name_edited = true;
            }
            Some(Field::BeforePattern) => self.before_pattern = text,
            Some(Field::AfterPattern) => self.after_pattern = text,
            Some(Field::Search) => {
                self.search_query = text;
                self.search_selected = 0;
            }
            Some(Field::Note(id)) => {
                if let Some(pair_id) = self.focus_pair_id() {
                    self.library.set_note(pair_id, id, &text);
                    self.persist();
                }
            }
            Some(Field::PinTitle(id)) => {
                if let Some(pair_id) = self.focus_pair_id() {
                    let body = self.pin_parts(id).1;
                    self.library.set_pin_text(pair_id, id, &text, &body);
                    self.persist();
                }
            }
            Some(Field::PinBody(id)) => {
                if let Some(pair_id) = self.focus_pair_id() {
                    let title = self.pin_parts(id).0;
                    self.library.set_pin_text(pair_id, id, &title, &text);
                    self.persist();
                }
            }
            None => {}
        }
    }

    fn insert_at_caret(&mut self, extra: &str) {
        let Some(field) = self.focused else {
            return;
        };
        let mut text = self.field_text();
        let room = field_limit(field).saturating_sub(text.chars().count());
        let extra: String = extra.chars().take(room).collect();
        if extra.is_empty() {
            return;
        }
        let caret = self.caret.min(text.chars().count());
        let byte: usize = text.chars().take(caret).map(char::len_utf8).sum();
        text.insert_str(byte, &extra);
        let added = extra.chars().count();
        self.set_field_text(text);
        self.caret = caret + added;
    }

    fn move_caret(&mut self, delta: i32) {
        let len = self.field_text().chars().count() as i32;
        self.caret = (self.caret as i32 + delta).clamp(0, len) as usize;
    }

    fn delete_caret(&mut self, direction: i32) {
        let mut text = self.field_text();
        let count = text.chars().count();
        let caret = self.caret.min(count);
        let (start, end, next) = if direction < 0 {
            if caret == 0 {
                return;
            }
            (caret - 1, caret, caret - 1)
        } else {
            if caret >= count {
                return;
            }
            (caret, caret + 1, caret)
        };
        let from: usize = text.chars().take(start).map(char::len_utf8).sum();
        let to: usize = text.chars().take(end).map(char::len_utf8).sum();
        text.replace_range(from..to, "");
        self.set_field_text(text);
        self.caret = next;
    }

    fn focus_pair_id(&self) -> Option<u64> {
        self.library.pairs.get(self.focus_pair).map(|pair| pair.id)
    }

    fn note_text(&self, id: u64) -> String {
        self.focus_pair_id()
            .and_then(|pair_id| self.library.pair_by_id(pair_id))
            .and_then(|pair| pair.notes.iter().find(|note| note.id == id))
            .map(|note| note.text.clone())
            .unwrap_or_default()
    }

    fn pin_parts(&self, id: u64) -> (String, String) {
        self.focus_pair_id()
            .and_then(|pair_id| self.library.pair_by_id(pair_id))
            .and_then(|pair| pair.pins.iter().find(|pin| pin.id == id))
            .map(|pin| (pin.title.clone(), pin.text.clone()))
            .unwrap_or_default()
    }

    fn remember_pair(&mut self, id: u64) {
        if let Some(index) = pair_position(&self.library, id) {
            self.selected_pair = index;
            self.focus_pair = index;
        }
    }

    fn find(&mut self, _: &Find, _: &mut Window, cx: &mut Context<Self>) {
        if self.screen != Screen::Viewer {
            return;
        }
        if self.search_open {
            self.close_search(cx);
        } else {
            self.open_search(cx);
        }
    }

    fn open_search(&mut self, cx: &mut Context<Self>) {
        self.cancel_placement_silent();
        self.search_scope = if self.viewer_groups.is_empty() {
            SearchScope::All
        } else {
            SearchScope::Current
        };
        self.search_query.clear();
        self.search_selected = 0;
        self.search_open = true;
        self.focused = Some(Field::Search);
        self.caret = 0;
        self.close_menus();
        cx.notify();
    }

    fn close_search(&mut self, cx: &mut Context<Self>) {
        self.search_open = false;
        self.scope_menu = false;
        if self.focused == Some(Field::Search) {
            self.focused = None;
        }
        cx.notify();
    }

    fn move_search(&mut self, delta: i32, cx: &mut Context<Self>) {
        let hits = self.search_hit_indexes();
        if hits.is_empty() {
            return;
        }
        let current = self.search_selected.min(hits.len() - 1);
        self.search_selected = step_index(current, hits.len(), delta);
        cx.notify();
    }

    fn search_hit_indexes(&self) -> Vec<usize> {
        let indexes = match self.search_scope {
            SearchScope::Current => self.visible_indices(),
            SearchScope::All => (0..self.library.pairs.len()).collect(),
        };
        let mut titles = Vec::new();
        let mut befores = Vec::new();
        let mut afters = Vec::new();
        for index in &indexes {
            let pair = &self.library.pairs[*index];
            titles.push(pair.name.clone());
            befores.push(self.filename(pair.before));
            afters.push(self.filename(pair.after));
        }
        let rows: Vec<(usize, &str, &str, &str)> = indexes
            .iter()
            .enumerate()
            .map(|(slot, index)| (*index, titles[slot].as_str(), befores[slot].as_str(), afters[slot].as_str()))
            .collect();
        search_pairs(&rows, &self.search_query)
    }

    fn filename(&self, photo_id: u64) -> String {
        self.library
            .photo(photo_id)
            .map(|photo| photo_label(photo))
            .unwrap_or_default()
    }

    fn open_search_result(&mut self, cx: &mut Context<Self>) {
        let hits = self.search_hit_indexes();
        let Some(&index) = hits.get(self.search_selected.min(hits.len().saturating_sub(1))) else {
            return;
        };
        if !self.visible_indices().contains(&index) {
            self.viewer_groups.clear();
            self.library_group = None;
        }
        self.focus_pair = index;
        self.selected_pair = index;
        self.selected_pin = None;
        self.screen = Screen::Viewer;
        self.search_open = false;
        self.scope_menu = false;
        if self.focused == Some(Field::Search) {
            self.focused = None;
        }
        cx.notify();
    }

    fn outside_visible(&self, index: usize) -> bool {
        self.search_scope == SearchScope::All && !self.visible_indices().contains(&index)
    }

    fn proposals(&self) -> Result<Vec<Proposal>, &'static str> {
        let files: Vec<(u64, String)> = self
            .library
            .unassigned()
            .into_iter()
            .map(|photo| (photo.id, photo_label(photo)))
            .collect();
        let refs: Vec<(u64, &str)> = files.iter().map(|(id, name)| (*id, name.as_str())).collect();
        propose_numbered(&refs, &self.before_pattern, &self.after_pattern)
    }

    fn open_auto_pair(&mut self, cx: &mut Context<Self>) {
        if self.library.photos().is_empty() {
            return;
        }
        self.auto_pair = true;
        self.focused = None;
        self.close_menus();
        self.search_open = false;
        cx.notify();
    }

    fn commit_auto_pair(&mut self, cx: &mut Context<Self>) {
        let Ok(proposals) = self.proposals() else {
            return;
        };
        let pairs = ready_pairs(&proposals);
        if pairs.is_empty() {
            return;
        }
        let start = self.library.pairs.len();
        let created = self.library.commit_matched(&pairs);
        if created == 0 {
            return;
        }
        self.selected_pair = start.min(self.library.pairs.len() - 1);
        self.focus_pair = self.selected_pair;
        self.auto_pair = false;
        self.focused = None;
        self.persist();
        cx.notify();
    }

    fn add_pair_note(&mut self, cx: &mut Context<Self>) {
        let Some(pair_id) = self.focus_pair_id() else {
            return;
        };
        if let Some(id) = self.library.add_note(pair_id) {
            self.focused = Some(Field::Note(id));
            self.caret = 0;
            self.selected_pin = None;
            self.persist();
        }
        cx.notify();
    }

    fn delete_pair_note(&mut self, note_id: u64, cx: &mut Context<Self>) {
        let Some(pair_id) = self.focus_pair_id() else {
            return;
        };
        if self.library.delete_note(pair_id, note_id) {
            if self.focused == Some(Field::Note(note_id)) {
                self.focused = None;
            }
            self.persist();
            cx.notify();
        }
    }

    fn delete_selected_pin(&mut self, pin_id: u64, cx: &mut Context<Self>) {
        let Some(pair_id) = self.focus_pair_id() else {
            return;
        };
        if self.library.delete_pin(pair_id, pin_id) {
            if self.selected_pin == Some(pin_id) {
                self.selected_pin = None;
            }
            if matches!(self.focused, Some(Field::PinTitle(id) | Field::PinBody(id)) if id == pin_id) {
                self.focused = None;
            }
            self.persist();
            cx.notify();
        }
    }

    fn begin_placement(&mut self, cx: &mut Context<Self>) {
        self.placing = true;
        self.focused = None;
        self.close_menus();
        cx.notify();
    }

    fn cancel_placement(&mut self, cx: &mut Context<Self>) {
        self.cancel_placement_silent();
        cx.notify();
    }

    fn cancel_placement_silent(&mut self) {
        let gesture = self.pin_drag.take();
        if let Some(PinGesture::Draw { id, .. }) = gesture
            && let Some(pair_id) = self.focus_pair_id()
        {
            self.library.delete_pin(pair_id, id);
            if self.selected_pin == Some(id) {
                self.selected_pin = None;
            }
            self.persist();
        } else if gesture.is_some() {
            self.persist();
        }
        self.placing = false;
    }

    fn begin_pin(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some((x, y)) = self.point_in_frame(position) else {
            return;
        };
        let Some(pair_id) = self.focus_pair_id() else {
            return;
        };
        let Some(id) = self.library.add_pin(pair_id, x, y, 0.0, 0.0) else {
            return;
        };
        self.pin_drag = Some(PinGesture::Draw { id, ax: x, ay: y });
        self.selected_pin = Some(id);
        self.placing = true;
        cx.notify();
    }

    fn begin_pin_move(&mut self, id: u64, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some((x, y)) = self.point_in_frame(position) else {
            return;
        };
        let Some(pair_id) = self.focus_pair_id() else {
            return;
        };
        let Some(pin) = self
            .library
            .pair_by_id(pair_id)
            .and_then(|pair| pair.pins.iter().find(|pin| pin.id == id))
        else {
            return;
        };
        let dx = x - pin.x;
        let dy = y - pin.y;
        self.selected_pin = Some(id);
        self.pin_drag = Some(PinGesture::Move { id, dx, dy });
        self.focused = None;
        cx.notify();
    }

    fn begin_pin_resize(&mut self, id: u64, cx: &mut Context<Self>) {
        self.selected_pin = Some(id);
        self.pin_drag = Some(PinGesture::Resize { id });
        self.focused = None;
        cx.notify();
    }

    fn update_pin_drag(&mut self, position: Point<Pixels>) -> bool {
        let Some((x, y)) = self.point_in_frame(position) else {
            return false;
        };
        let Some(pair_id) = self.focus_pair_id() else {
            return false;
        };
        let Some(gesture) = self.pin_drag.clone() else {
            return false;
        };
        match gesture {
            PinGesture::Draw { id, ax, ay } => {
                let (rx, ry, mut rw, mut rh) = norm_rect(ax, ay, x, y);
                if rw < 0.012 && rh < 0.012 {
                    rw = 0.0;
                    rh = 0.0;
                }
                self.library.set_pin_bounds(pair_id, id, rx, ry, rw, rh)
            }
            PinGesture::Move { id, dx, dy } => {
                let Some(pin) = self
                    .library
                    .pair_by_id(pair_id)
                    .and_then(|pair| pair.pins.iter().find(|pin| pin.id == id))
                else {
                    return false;
                };
                let w = pin.w;
                let h = pin.h;
                let nx = (x - dx).clamp(0.0, (1.0 - w).max(0.0));
                let ny = (y - dy).clamp(0.0, (1.0 - h).max(0.0));
                self.library.set_pin_bounds(pair_id, id, nx, ny, w, h)
            }
            PinGesture::Resize { id } => {
                let Some(pin) = self
                    .library
                    .pair_by_id(pair_id)
                    .and_then(|pair| pair.pins.iter().find(|pin| pin.id == id))
                else {
                    return false;
                };
                let w = (x - pin.x).clamp(0.0, 1.0 - pin.x);
                let h = (y - pin.y).clamp(0.0, 1.0 - pin.y);
                self.library.set_pin_bounds(pair_id, id, pin.x, pin.y, w, h)
            }
        }
    }

    fn finish_pin_drag(&mut self) {
        let Some(gesture) = self.pin_drag.take() else {
            return;
        };
        if let PinGesture::Draw { id, .. } = gesture {
            self.placing = false;
            self.focused = Some(Field::PinTitle(id));
            self.caret = 0;
            self.selected_pin = Some(id);
        }
        self.persist();
    }

    fn point_in_frame(&self, position: Point<Pixels>) -> Option<(f32, f32)> {
        let bounds = self.frame.get()?;
        let width = px_f(bounds.size.width);
        let height = px_f(bounds.size.height);
        if width < 1.0 || height < 1.0 {
            return None;
        }
        let x = ((px_f(position.x) - px_f(bounds.origin.x)) / width).clamp(0.0, 1.0);
        let y = ((px_f(position.y) - px_f(bounds.origin.y)) / height).clamp(0.0, 1.0);
        Some((x, y))
    }

    fn apply_blend(&mut self, position: Point<Pixels>) -> bool {
        let Some(bounds) = self.blend_frame.get() else {
            return false;
        };
        let width = px_f(bounds.size.width);
        if width < 1.0 {
            return false;
        }
        let x = ((px_f(position.x) - px_f(bounds.origin.x)) / width).clamp(0.0, 1.0);
        if (self.blend - x).abs() < 0.001 {
            return false;
        }
        self.blend = x;
        true
    }

    fn begin_blend(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        self.blend_drag = true;
        self.close_menus();
        self.apply_blend(position);
        cx.notify();
    }

    fn set_compare_mode(&mut self, mode: CompareMode, cx: &mut Context<Self>) {
        self.compare_mode = mode;
        self.mode_menu = false;
        self.persist();
        cx.notify();
    }

    fn listed_row(&self, position: usize) -> Option<PairRow> {
        let index = *self.listed_pairs().get(position)?;
        let pair = self.library.pairs.get(index)?;
        let before = self.library.photo(pair.before)?.path.clone();
        let after = self.library.photo(pair.after)?.path.clone();
        Some(PairRow {
            index,
            position,
            pair_id: pair.id,
            name: pair.name.clone(),
            label: self.library.group_label(pair.id),
            before,
            after,
            before_id: pair.before,
            after_id: pair.after,
        })
    }

    fn listed_row_by_id(&self, pair_id: u64) -> Option<PairRow> {
        let position = self.listed_pairs().into_iter().position(|index| {
            self.library
                .pairs
                .get(index)
                .is_some_and(|pair| pair.id == pair_id)
        })?;
        self.listed_row(position)
    }

    fn begin_row_drag(&mut self, row: &PairRow, cursor: Point<Pixels>, cx: &mut Context<Self>) {
        let bounds = self.row_bounds.borrow().get(row.position).cloned();
        let (grab_x, grab_y, width) = if let Some(bounds) = bounds {
            (
                px_f(cursor.x) - px_f(bounds.origin.x),
                px_f(cursor.y) - px_f(bounds.origin.y),
                px_f(bounds.size.width).max(240.0),
            )
        } else {
            (28.0, 24.0, 640.0)
        };
        self.row_drag = Some(RowDrag {
            pair_id: row.pair_id,
            from: row.position,
            target: row.position,
            cursor,
            origin: cursor,
            grab_x,
            grab_y,
            width,
            moved: false,
        });
        self.close_menus();
        cx.notify();
    }

    fn update_row_target(&mut self, cursor: Point<Pixels>) -> bool {
        let count = self.listed_pairs().len();
        let y = px_f(cursor.y);
        let target = self.row_index_at(y, count);
        let mut changed = self.autoscroll_library(y);
        if let Some(drag) = self.row_drag.as_mut() {
            let dx = px_f(cursor.x) - px_f(drag.origin.x);
            let dy = px_f(cursor.y) - px_f(drag.origin.y);
            if dx * dx + dy * dy > 16.0 {
                drag.moved = true;
            }
            if drag.target != target || px_f(drag.cursor.y) != y {
                drag.target = target;
                drag.cursor = cursor;
                changed = true;
            }
        }
        changed
    }

    fn row_index_at(&self, cursor_y: f32, count: usize) -> usize {
        if count == 0 {
            return 0;
        }
        let handle = self.library_scroll.0.borrow().base_handle.clone();
        let item_h = self
            .library_scroll
            .0
            .borrow()
            .last_item_size
            .map(|size| px_f(size.contents.height) / count as f32)
            .filter(|height| *height > 1.0)
            .unwrap_or(PAIR_ROW_H);
        let origin_y = px_f(handle.bounds().origin.y);
        let content_y = cursor_y - origin_y - px_f(handle.offset().y);
        row_target(content_y, item_h, count)
    }

    fn autoscroll_library(&self, cursor_y: f32) -> bool {
        let handle = self.library_scroll.0.borrow().base_handle.clone();
        let bounds = handle.bounds();
        let height = px_f(bounds.size.height);
        if height < 1.0 {
            return false;
        }
        let top = px_f(bounds.origin.y);
        let bottom = top + height;
        let mut offset = handle.offset();
        let max_y = px_f(handle.max_offset().y).min(0.0);
        let y = px_f(offset.y);
        let next = if cursor_y < top + 48.0 {
            (y + 18.0).min(0.0)
        } else if cursor_y > bottom - 48.0 {
            (y - 18.0).max(max_y)
        } else {
            return false;
        };
        if (next - y).abs() < 0.5 {
            return false;
        }
        offset.y = px(next);
        handle.set_offset(offset);
        true
    }

    fn finish_row_drag(&mut self) {
        let Some(drag) = self.row_drag.take() else {
            return;
        };
        if !drag.moved || drag.from == drag.target {
            return;
        }
        self.block_row_open = true;
        let listed = self.listed_pairs();
        let selected_id = self.library.pairs.get(self.selected_pair).map(|pair| pair.id);
        let focus_id = self.library.pairs.get(self.focus_pair).map(|pair| pair.id);
        if self.library.reorder_visible(&listed, drag.from, drag.target) {
            if let Some(id) = selected_id {
                self.remember_pair(id);
            }
            if let Some(id) = focus_id && selected_id != Some(id) {
                if let Some(index) = pair_position(&self.library, id) {
                    self.focus_pair = index;
                }
            }
            self.persist();
        }
    }

    fn apply_pair_order(&mut self, order: PairOrder, cx: &mut Context<Self>) {
        let focus_id = self.library.pairs.get(self.focus_pair).map(|pair| pair.id);
        let selected_id = self
            .library
            .pairs
            .get(self.selected_pair)
            .map(|pair| pair.id);
        let listed = self.listed_pairs();
        self.library.sort_visible(&listed, order);
        self.restore_pair_focus(focus_id, selected_id);
        self.order_menu = false;
        self.persist();
        cx.notify();
    }

    fn order_button(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let label = self.library.order().label();
        let anchor = self.order_anchor.clone();
        div()
            .id("order-toggle")
            .relative()
            .h(px(34.))
            .px(px(14.))
            .rounded_full()
            .flex()
            .items_center()
            .gap(px(8.))
            .text_sm()
            .font_weight(FontWeight::MEDIUM)
            .bg(rgb(0x2C2C2E))
            .text_color(rgb(TEXT))
            .cursor(CursorStyle::PointingHand)
            .hover(|style| style.bg(rgb(HOVER)))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                let open = !this.order_menu;
                this.close_menus();
                this.order_menu = open;
                cx.stop_propagation();
                cx.notify();
            }))
            .child(
                canvas(
                    |bounds, _, _| bounds,
                    move |bounds, _, _, _| {
                        anchor.set(Some(bounds));
                    },
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .child(icon(SORT_ICON, 15., TEXT))
            .child(label)
            .child(div().text_xs().text_color(rgb(MUTED)).child("▾"))
            .into_any_element()
    }

    fn order_menu_layer(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let current = self.library.order();
        let (left, top) = self
            .order_anchor
            .get()
            .map(|bounds| {
                let left = (px_f(bounds.origin.x) + px_f(bounds.size.width) - 188.0).max(8.0);
                let top = px_f(bounds.origin.y) + px_f(bounds.size.height) + 6.0;
                (left, top)
            })
            .unwrap_or((8.0, HEADER + 70.0));
        div()
            .id("order-backdrop")
            .absolute()
            .inset_0()
            .occlude()
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.order_menu = false;
                cx.stop_propagation();
                cx.notify();
            }))
            .child(
                div()
                    .id("order-menu")
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .w(px(188.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(rgb(LINE))
                    .bg(rgb(MENU))
                    .py(px(4.))
                    .occlude()
                    .on_click(|_: &ClickEvent, _, cx| cx.stop_propagation())
                    .child(order_option(PairOrder::Name, current, cx))
                    .child(order_option(PairOrder::NameDesc, current, cx))
                    .child(order_option(PairOrder::Oldest, current, cx))
                    .child(order_option(PairOrder::Newest, current, cx))
                    .child(order_option(PairOrder::Custom, current, cx)),
            )
            .into_any_element()
    }

    fn drag_hint(&self) -> Option<String> {
        let drag = self.row_drag.as_ref()?;
        if !drag.moved {
            return None;
        }
        let name = self
            .library
            .pair_by_id(drag.pair_id)
            .map(|pair| short_name(&pair.name))
            .unwrap_or_else(|| "Pair".to_string());
        Some(format!("Move {name} to position {}", drag.target + 1))
    }

    fn clear_paste_state(&mut self) {
        self.paste_undo = None;
        self.toast = None;
        self.discard_staged();
    }

    fn discard_staged(&mut self) {
        let staged = std::mem::take(&mut self.staged);
        for path in staged {
            self.remove_if_unreferenced(&path);
        }
    }

    fn remove_if_unreferenced(&mut self, path: &Path) {
        if self.paste_in_use(path) {
            return;
        }
        let _ = std::fs::remove_file(path);
        self.staged.retain(|item| item != path);
    }

    fn paste_in_use(&self, path: &Path) -> bool {
        self.paste_before.as_deref() == Some(path)
            || self.paste_after.as_deref() == Some(path)
            || matches!(&self.edit_before, Some(SlotValue::Paste(item)) if item == path)
            || matches!(&self.edit_after, Some(SlotValue::Paste(item)) if item == path)
            || self.library.photos().iter().any(|photo| photo.path == path)
    }

    fn adopt_loose(&mut self, path: PathBuf) -> Option<u64> {
        if let Some(photo) = self.library.photos().iter().find(|photo| photo.path == path) {
            if self.library.pair_index(photo.id).is_some() {
                return None;
            }
            return Some(photo.id);
        }
        self.library.insert_clipboard(path)
    }

    fn adopt_any(&mut self, path: PathBuf) -> Option<u64> {
        if let Some(photo) = self.library.photos().iter().find(|photo| photo.path == path) {
            return Some(photo.id);
        }
        self.library.insert_clipboard(path)
    }

    fn paste_image(&mut self, _: &PasteImage, _: &mut Window, cx: &mut Context<Self>) {
        if self.text_focused() || !matches!(self.screen, Screen::Create | Screen::Edit) {
            return;
        }
        let Some(item) = cx.read_from_clipboard() else {
            self.fail_paste("Clipboard has no image.");
            cx.notify();
            return;
        };
        let Some(image) = item.entries().iter().find_map(|entry| match entry {
            gpui::ClipboardEntry::Image(image) => Some(image.clone()),
            _ => None,
        }) else {
            self.fail_paste("Clipboard has no image.");
            cx.notify();
            return;
        };
        let ext = image.format().extension();
        if !looks_like_image(image.bytes(), ext) {
            self.fail_paste("That image can't be used.");
            cx.notify();
            return;
        }
        let path = match write_asset(image.bytes(), ext) {
            Ok(path) => path,
            Err(_) => {
                self.fail_paste("Couldn't save that image.");
                cx.notify();
                return;
            }
        };
        self.assign_paste(path);
        cx.notify();
    }

    fn assign_paste(&mut self, path: PathBuf) {
        let side = self.slot;
        match self.screen {
            Screen::Create => {
                let previous = match side {
                    SlotSide::Before => self.paste_before.clone(),
                    SlotSide::After => self.paste_after.clone(),
                };
                match side {
                    SlotSide::Before => self.paste_before = Some(path.clone()),
                    SlotSide::After => self.paste_after = Some(path.clone()),
                }
                if side == SlotSide::Before && !self.name_edited && self.pair_name.is_empty() {
                    self.pair_name = "Clipboard".to_string();
                }
                self.paste_undo = Some(PasteUndo::Create {
                    side,
                    previous,
                    current: path.clone(),
                });
            }
            Screen::Edit => {
                let previous = match side {
                    SlotSide::Before => self.edit_before.clone(),
                    SlotSide::After => self.edit_after.clone(),
                };
                let value = Some(SlotValue::Paste(path.clone()));
                match side {
                    SlotSide::Before => self.edit_before = value,
                    SlotSide::After => self.edit_after = value,
                }
                self.paste_undo = Some(PasteUndo::Edit {
                    side,
                    previous,
                    current: path.clone(),
                });
            }
            _ => return,
        }
        self.staged.push(path);
        self.toast = Some(Toast {
            message: format!("Pasted image into {}.", side_name(side)),
            undo: true,
        });
    }

    fn undo_paste(&mut self, cx: &mut Context<Self>) {
        let Some(undo) = self.paste_undo.take() else {
            return;
        };
        match undo {
            PasteUndo::Create { side, previous, current } => {
                match side {
                    SlotSide::Before => self.paste_before = previous,
                    SlotSide::After => self.paste_after = previous,
                }
                self.remove_if_unreferenced(&current);
            }
            PasteUndo::Edit { side, previous, current } => {
                match side {
                    SlotSide::Before => self.edit_before = previous,
                    SlotSide::After => self.edit_after = previous,
                }
                self.remove_if_unreferenced(&current);
            }
        }
        self.toast = None;
        cx.notify();
    }

    fn fail_paste(&mut self, message: &str) {
        self.toast = Some(Toast {
            message: message.to_string(),
            undo: false,
        });
    }

    fn select_slot(&mut self, side: SlotSide, cx: &mut Context<Self>) {
        self.slot = side;
        if matches!(self.focused, Some(Field::PairName | Field::GroupName)) {
            self.focused = None;
        }
        cx.notify();
    }

    fn create_slot(&self, side: SlotSide) -> Option<(PathBuf, String)> {
        let pasted = match side {
            SlotSide::Before => self.paste_before.clone(),
            SlotSide::After => self.paste_after.clone(),
        };
        if let Some(path) = pasted {
            return Some((path, "Clipboard image".to_string()));
        }
        let id = match side {
            SlotSide::Before => self.library.draft_before,
            SlotSide::After => self.library.draft_after,
        }?;
        let photo = self.library.photo(id)?;
        Some((photo.path.clone(), source_label(photo)))
    }

    fn edit_slot(&self, side: SlotSide) -> Option<(PathBuf, String)> {
        let value = match side {
            SlotSide::Before => self.edit_before.clone(),
            SlotSide::After => self.edit_after.clone(),
        }?;
        match value {
            SlotValue::Paste(path) => Some((path, "Clipboard image".to_string())),
            SlotValue::Photo(id) => {
                let photo = self.library.photo(id)?;
                Some((photo.path.clone(), source_label(photo)))
            }
        }
    }

    fn open_edit(&mut self, pair_id: u64, return_to: Screen, cx: &mut Context<Self>) {
        self.cancel_placement_silent();
        self.search_open = false;
        self.auto_pair = false;
        let Some(pair) = self.library.pair_by_id(pair_id) else {
            return;
        };
        self.pair_name = pair.name.clone();
        self.name_edited = true;
        self.edit_before = Some(SlotValue::Photo(pair.before));
        self.edit_after = Some(SlotValue::Photo(pair.after));
        self.editing = Some(pair_id);
        self.edit_return = return_to;
        self.slot = SlotSide::After;
        self.focused = None;
        self.paste_before = None;
        self.paste_after = None;
        self.clear_paste_state();
        self.screen = Screen::Edit;
        self.close_menus();
        if let Some(index) = pair_position(&self.library, pair_id) {
            self.selected_pair = index;
            self.focus_pair = index;
        }
        cx.notify();
    }

    fn cancel_edit(&mut self, cx: &mut Context<Self>) {
        let return_to = self.edit_return;
        self.editing = None;
        self.edit_before = None;
        self.edit_after = None;
        self.reset_editor_text();
        self.clear_paste_state();
        self.screen = return_to;
        cx.notify();
    }

    fn save_edit(&mut self, cx: &mut Context<Self>) {
        let Some(pair_id) = self.editing else {
            return;
        };
        let Some(before) = self.materialize(self.edit_before.clone(), Some(pair_id)) else {
            self.fail_paste("Before needs an image.");
            cx.notify();
            return;
        };
        let Some(after) = self.materialize(self.edit_after.clone(), Some(pair_id)) else {
            self.fail_paste("After needs an image.");
            cx.notify();
            return;
        };
        if !self.library.update_pair(pair_id, &self.pair_name, before, after) {
            self.fail_paste("Those images are already used in another pair.");
            cx.notify();
            return;
        }
        let return_to = self.edit_return;
        self.editing = None;
        self.edit_before = None;
        self.edit_after = None;
        self.staged.clear();
        self.paste_undo = None;
        self.toast = None;
        self.reset_editor_text();
        self.remember_pair(pair_id);
        self.screen = return_to;
        self.persist();
        cx.notify();
    }

    fn materialize(&mut self, slot: Option<SlotValue>, pair_id: Option<u64>) -> Option<u64> {
        match slot? {
            SlotValue::Photo(id) => {
                if self.library.photo(id).is_none() {
                    return None;
                }
                if let Some(index) = self.library.pair_index(id)
                    && pair_id != Some(self.library.pairs[index].id)
                {
                    return None;
                }
                Some(id)
            }
            SlotValue::Paste(path) => self.adopt_any(path),
        }
    }
}

fn object_fit(mode: FitMode) -> ObjectFit {
    match mode {
        FitMode::Fit => ObjectFit::Contain,
        FitMode::Fill => ObjectFit::Cover,
    }
}

impl Render for Studio {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let screen = self.screen;
        let empty = self.library.photos().is_empty();
        let body = match screen {
            Screen::Library | Screen::NewGroup if empty => self.empty_state(cx),
            Screen::Library | Screen::NewGroup => self.library_screen(window, cx),
            Screen::Create => self.create_screen(window, cx),
            Screen::Viewer => self.viewer(window, cx),
            Screen::Edit => self.edit_screen(cx),
        };
        let group_modal = (screen == Screen::NewGroup).then(|| self.new_group_overlay(window, cx));
        let delete_modal =
            (self.confirm_delete && screen == Screen::Library).then(|| self.delete_overlay(cx));
        let groups_menu =
            (self.groups_menu && screen == Screen::Viewer).then(|| self.groups_overlay(cx));
        let search = (self.search_open && screen == Screen::Viewer).then(|| self.search_overlay(cx));
        let auto_pair = (self.auto_pair && screen == Screen::Library).then(|| self.auto_overlay(cx));
        let order_menu =
            (self.order_menu && screen == Screen::Library && !empty).then(|| self.order_menu_layer(cx));
        let dragging = self.row_drag.as_ref().is_some_and(|drag| drag.moved);
        let drag_layer = dragging.then(|| self.drag_layer(cx));
        let toast = self.toast.as_ref().map(|toast| toast.message.clone());
        let toast_undo = self.toast.as_ref().is_some_and(|toast| toast.undo);
        let show_toast = toast.is_some() && matches!(screen, Screen::Create | Screen::Edit);
        div()
            .id("compare-root")
            .track_focus(&self.focus)
            .key_context("Compare")
            .on_action(cx.listener(Self::add_photos))
            .on_action(cx.listener(Self::next_group))
            .on_action(cx.listener(Self::prev_group))
            .on_action(cx.listener(Self::nudge_left))
            .on_action(cx.listener(Self::nudge_right))
            .on_action(cx.listener(Self::snap_left))
            .on_action(cx.listener(Self::snap_right))
            .on_action(cx.listener(Self::open_viewer))
            .on_action(cx.listener(Self::toggle_mode))
            .on_action(cx.listener(Self::unlink))
            .on_action(cx.listener(Self::find))
            .on_action(cx.listener(Self::paste_image))
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                this.track_move(event.position, cx);
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| this.track_up(cx)),
            )
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                this.ingest(paths.paths().to_vec(), cx);
            }))
            .drag_over::<ExternalPaths>(|style, _, _, _| style.bg(rgb(0x1C1C1E)))
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .font_family("Helvetica Neue")
            .child(self.header(cx))
            .child(body)
            .children(groups_menu)
            .child(self.footer(cx))
            .children(order_menu)
            .children(search)
            .children(auto_pair)
            .children(group_modal)
            .children(delete_modal)
            .children(drag_layer)
            .when(show_toast, |this| {
                this.child(toast_bar(toast.unwrap_or_default(), toast_undo, cx))
            })
    }
}

impl Studio {
    fn header(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let center = match self.screen {
            Screen::Library | Screen::NewGroup if self.library.photos().is_empty() => {
                SharedString::from("Untitled")
            }
            Screen::Library | Screen::NewGroup => SharedString::from("Images"),
            Screen::Create => SharedString::from("Create pair"),
            Screen::Edit => SharedString::from("Edit pair"),
            Screen::Viewer => {
                let index = self.focus_pair.min(self.library.pairs.len().saturating_sub(1));
                self.library
                    .pairs
                    .get(index)
                    .map(|pair| SharedString::from(pair.name.clone()))
                    .unwrap_or_else(|| SharedString::from("Untitled"))
            }
        };
        let actions = self.header_actions(cx);
        div()
            .relative()
            .h(px(HEADER))
            .w_full()
            .flex_shrink_0()
            .bg(rgb(HEADER_BG))
            .border_b_1()
            .border_color(rgb(LINE))
            .window_control_area(WindowControlArea::Drag)
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(center),
            )
            .child(
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_between()
                    .pl(px(120.))
                    .pr(px(22.))
                    .child(if self.screen == Screen::Viewer {
                        self.back_button(cx)
                    } else {
                        div().into_any_element()
                    })
                    .child(actions),
            )
            .into_any_element()
    }

    fn header_actions(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        match self.screen {
            Screen::Create => text_button(
                "cancel",
                "Cancel",
                cx.listener(|this, _: &ClickEvent, _, cx| this.cancel_create(cx)),
            ),
            Screen::Edit => text_button(
                "cancel-edit",
                "Cancel",
                cx.listener(|this, _: &ClickEvent, _, cx| this.cancel_edit(cx)),
            ),
            Screen::Viewer => div()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(icon_button(
                    "add-viewer",
                    cx.listener(|this, _: &ClickEvent, _, cx| this.pick_files(cx)),
                ))
                .child(self.search_button(cx))
                .child(self.groups_button(cx))
                .child(self.notes_button(cx))
                .child(text_button(
                    "organize",
                    "Organize",
                    cx.listener(|this, _: &ClickEvent, _, cx| this.show_library(cx)),
                ))
                .into_any_element(),
            Screen::Library | Screen::NewGroup => {
                let editing = self.screen == Screen::Library && self.arrange;
                let (pairs, photos) = if editing {
                    self.visible_pick_targets()
                } else {
                    (Vec::new(), Vec::new())
                };
                let select_label =
                    select_all_label(&pairs, &photos, &self.picked_pairs, &self.picked_photos);
                let can_select = !pairs.is_empty() || !photos.is_empty();
                let delete_count = self.picked_pairs.len() + self.picked_photos.len();
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .when(editing, |this| {
                        this.child(edit_text_button(
                            "select-all",
                            SharedString::from(select_label),
                            false,
                            can_select,
                            cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_select_all(cx)),
                        ))
                        .child(edit_text_button(
                            "delete-selected",
                            SharedString::from(format!("Delete ({delete_count})")),
                            true,
                            delete_count > 0,
                            cx.listener(|this, _: &ClickEvent, _, cx| this.ask_delete_picked(cx)),
                        ))
                    })
                    .child(icon_button(
                        "add-library",
                        cx.listener(|this, _: &ClickEvent, _, cx| this.pick_files(cx)),
                    ))
                    .child(self.pencil_button(cx))
                    .into_any_element()
            }
        }
    }

    fn pencil_button(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let on = self.arrange;
        div()
            .id("arrange-toggle")
            .w(px(32.))
            .h(px(32.))
            .rounded(px(8.))
            .border_1()
            .border_color(rgb(if on { BLUE } else { 0x6E6E73 }))
            .bg(rgb(if on { 0x1A2744 } else { HEADER_BG }))
            .flex()
            .items_center()
            .justify_center()
            .cursor(CursorStyle::PointingHand)
            .hover(|style| style.bg(rgb(if on { 0x243656 } else { HOVER })))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.arrange = !this.arrange;
                if !this.arrange {
                    this.confirm_delete = false;
                    this.picked_pairs.clear();
                    this.picked_photos.clear();
                }
                cx.notify();
            }))
            .child(icon(PENCIL_ICON, 15., if on { 0xFFFFFF } else { TEXT }))
            .into_any_element()
    }

    fn footer(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let photos = self.library.photos().len();
        let pairs = self.library.pairs.len();
        let bar = div()
            .h(px(FOOTER))
            .w_full()
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_between()
            .px(px(22.))
            .border_t_1()
            .border_color(rgb(LINE))
            .text_sm()
            .text_color(rgb(MUTED));

        let bar = match self.screen {
            Screen::Viewer => {
                let visible = self.visible_indices();
                let len = visible.len();
                let position = visible
                    .iter()
                    .position(|index| *index == self.focus_pair)
                    .map(|index| index + 1)
                    .unwrap_or(0);
                bar.child(SharedString::from(format!(
                    "{position} / {len} {}",
                    plural(len, "pair", "pairs")
                )))
                .child(self.viewer_controls(cx))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child("↑ ↓")
                        .child("Navigate")
                        .child(self.step_buttons(cx)),
                )
            }
            Screen::Library | Screen::NewGroup if photos == 0 => bar
                .child("0 images")
                .child("PNG · JPG · WEBP"),
            Screen::Library | Screen::NewGroup if pairs == 0 => bar
                .child(SharedString::from(format!(
                    "{photos} {}",
                    plural(photos, "image", "images"),
                )))
                .child("Select images to link"),
            Screen::Library | Screen::NewGroup => {
                let hint = self.drag_hint();
                let custom = self.library.custom_order() || hint.is_some();
                let left = if custom {
                    format!(
                        "{pairs} {} · Custom order",
                        plural(pairs, "pair", "pairs")
                    )
                } else {
                    format!(
                        "{pairs} {} · {photos} {}",
                        plural(pairs, "pair", "pairs"),
                        plural(photos, "image", "images"),
                    )
                };
                bar.child(SharedString::from(left)).child(SharedString::from(
                    hint.unwrap_or_else(|| "Select images to link".to_string()),
                ))
            }
            Screen::Edit => bar
                .child(SharedString::from(format!(
                    "{} · Paste into selected slot",
                    shortcut("V")
                )))
                .child(pill(
                    "save-pair",
                    "Save pair",
                    None,
                    self.edit_slot(SlotSide::Before).is_some() && self.edit_slot(SlotSide::After).is_some(),
                    cx.listener(|this, _: &ClickEvent, _, cx| this.save_edit(cx)),
                )),
            Screen::Create => {
                let selected = usize::from(self.library.draft_before.is_some())
                    + usize::from(self.library.draft_after.is_some());
                bar.child(SharedString::from(format!(
                    "{pairs} {} · {photos} {}",
                    plural(pairs, "pair", "pairs"),
                    plural(photos, "image", "images"),
                )))
                .child(SharedString::from(format!(
                    "{selected} {}",
                    plural(selected, "selected", "selected"),
                )))
            }
        };
        bar.into_any_element()
    }

    fn fit_control(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let label = match self.fit {
            FitMode::Fit => "Fit",
            FitMode::Fill => "Fill",
        };
        let open = self.fit_menu;
        div()
            .relative()
            .child(
                div()
                    .id("fit-toggle")
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .cursor(CursorStyle::PointingHand)
                    .text_color(rgb(TEXT))
                    .hover(|style| style.opacity(0.72))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.fit_menu = !this.fit_menu;
                        if this.fit_menu {
                            this.groups_menu = false;
                            this.mode_menu = false;
                        }
                        cx.notify();
                    }))
                    .child(label)
                    .child(div().text_xs().text_color(rgb(MUTED)).child("▾")),
            )
            .when(open, |this| {
                this.child(
                    div()
                        .absolute()
                        .bottom(px(28.))
                        .left(px(-28.))
                        .w(px(148.))
                        .rounded(px(12.))
                        .border_1()
                        .border_color(rgb(LINE))
                        .bg(rgb(0x242424))
                        .py(px(4.))
                        .child(fit_option("Fit", self.fit == FitMode::Fit, FitMode::Fit, cx))
                        .child(fit_option("Fill", self.fit == FitMode::Fill, FitMode::Fill, cx)),
                )
            })
            .into_any_element()
    }

    fn empty_state(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        div()
            .flex_1()
            .min_h(px(0.))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(8.))
            .child(
                svg()
                    .data(DROP_ICON)
                    .w(px(72.))
                    .h(px(64.))
                    .mb(px(18.)),
            )
            .child(
                div()
                    .text_size(px(28.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Drop screenshots here"),
            )
            .child(
                div()
                    .text_color(rgb(MUTED))
                    .mb(px(16.))
                    .child("Import any number of images."),
            )
            .child(pill(
                "choose-images",
                "Choose images",
                None,
                true,
                cx.listener(|this, _: &ClickEvent, _, cx| this.pick_files(cx)),
            ))
            .child(
                div()
                    .mt(px(16.))
                    .text_sm()
                    .text_color(rgb(MUTED))
                    .child("Link two images to create a before / after pair."),
            )
            .into_any_element()
    }

    fn library_screen(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let viewport_w = px_f(window.viewport_size().width);
        if self.library.pairs.is_empty() {
            let unpaired = self.library.unassigned().len();
            return div()
                .flex_1()
                .min_h(px(0.))
                .flex()
                .flex_col()
                .child(self.images_heading(cx))
                .child(
                    div()
                        .flex_1()
                        .min_h(px(0.))
                        .overflow_hidden()
                        .when(unpaired > 0, |this| {
                            this.child(self.photo_list(
                                "library-photos",
                                false,
                                viewport_w,
                                false,
                                cx,
                            ))
                        }),
                )
                .into_any_element();
        }

        div()
            .flex_1()
            .min_h(px(0.))
            .flex()
            .child(self.group_sidebar(cx))
            .child(self.library_main(viewport_w, cx))
            .into_any_element()
    }

    fn group_sidebar(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let all_count = self.library.pairs.len();
        let selected = self.library_group;
        let groups: Vec<_> = self
            .library
            .groups()
            .iter()
            .map(|group| (group.id, group.name.clone(), group.pair_count()))
            .collect();
        div()
            .w(px(220.))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(rgb(LINE))
            .child(
                div()
                    .id("sidebar-scroll")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .px(px(10.))
                    .py(px(14.))
                    .gap(px(2.))
                    .child(sidebar_row(
                        "side-all".into(),
                        Some(STACK_ICON),
                        "All pairs".into(),
                        all_count,
                        selected.is_none(),
                        cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.select_library_group(None, cx);
                        }),
                    ))
                    .child(
                        div()
                            .px(px(10.))
                            .pt(px(16.))
                            .pb(px(6.))
                            .text_xs()
                            .text_color(rgb(MUTED))
                            .child("Groups"),
                    )
                    .children(groups.into_iter().map(|(id, name, count)| {
                        sidebar_row(
                            SharedString::from(format!("side-{id}")),
                            Some(FOLDER_ICON),
                            SharedString::from(name),
                            count,
                            selected == Some(id),
                            cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.select_library_group(Some(id), cx);
                            }),
                        )
                    }))
                    .child(
                        div()
                            .id("new-group")
                            .h(px(34.))
                            .px(px(10.))
                            .mt(px(6.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .rounded(px(8.))
                            .cursor(CursorStyle::PointingHand)
                            .text_sm()
                            .hover(|style| style.bg(rgb(0x242424)))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.open_new_group(cx);
                            }))
                            .child(icon(PLUS_ICON, 14., TEXT))
                            .child("New group"),
                    ),
            )
            .into_any_element()
    }

    fn library_main(&mut self, viewport_w: f32, cx: &mut Context<Self>) -> gpui::AnyElement {
        let listed = self.listed_pairs();
        let unpaired = self.library.unassigned().len();
        let title = match self.library_group {
            Some(id) => self
                .library
                .group(id)
                .map(|group| SharedString::from(group.name.clone()))
                .unwrap_or_else(|| SharedString::from("All pairs")),
            None => SharedString::from("All pairs"),
        };
        let pair_count = listed.len();
        let image_count = if self.library_group.is_some() {
            pair_count * 2
        } else {
            self.library.photos().len()
        };
        let can_view = pair_count > 0;
        let loose = unpaired > 0;

        div()
            .flex_1()
            .min_w(px(0.))
            .min_h(px(0.))
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(28.))
                    .pt(px(22.))
                    .pb(px(12.))
                    .flex()
                    .items_start()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(px(28.))
                                    .font_weight(FontWeight::BOLD)
                                    .child(title),
                            )
                            .child(
                                div().text_sm().text_color(rgb(MUTED)).child(SharedString::from(
                                    format!(
                                        "{pair_count} {} · {image_count} {}",
                                        plural(pair_count, "pair", "pairs"),
                                        plural(image_count, "image", "images"),
                                    ),
                                )),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .when(unpaired >= 2, |this| {
                                this.child(pill(
                                    "open-create",
                                    "Create pair",
                                    Some(PAIR_ICON),
                                    true,
                                    cx.listener(|this, _: &ClickEvent, _, cx| this.open_create(cx)),
                                ))
                            })
                            .child(quiet_pill(
                                "auto-pair",
                                "Auto pair",
                                Some(LINK_ICON),
                                true,
                                cx.listener(|this, _: &ClickEvent, _, cx| this.open_auto_pair(cx)),
                            ))
                            .when(pair_count > 0, |this| this.child(self.order_button(cx)))
                            .child(quiet_pill(
                                "view-pairs",
                                "View pairs",
                                Some(SCREEN_ICON),
                                can_view,
                                cx.listener(|this, _: &ClickEvent, _, cx| this.enter_viewer(cx)),
                            )),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_hidden()
                    .child(self.pair_list(cx)),
            )
            .when(loose, |this| {
                let open = self.unpaired_open;
                let columns = grid_columns(
                    viewport_w,
                    LIBRARY_CARD_W,
                    GRID_GAP,
                    LIBRARY_CHROME + SIDEBAR_W,
                );
                let row_h = LIBRARY_IMAGE_H + 44.0 + GRID_GAP;
                let visible_rows = unpaired.div_ceil(columns).clamp(1, 3);
                let grid_h = row_h * visible_rows as f32;
                this.child(
                    div()
                        .flex_shrink_0()
                        .flex()
                        .flex_col()
                        .child(self.unpaired_header(unpaired, cx))
                        .when(open, |this| {
                            this.child(
                                div()
                                    .h(px(grid_h + UNPAIRED_PAD))
                                    .overflow_hidden()
                                    .flex()
                                    .flex_col()
                                    .child(div().h(px(UNPAIRED_PAD)).flex_shrink_0())
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_h(px(0.))
                                            .overflow_hidden()
                                            .child(self.photo_list(
                                                "loose-photos",
                                                false,
                                                viewport_w,
                                                true,
                                                cx,
                                            )),
                                    ),
                            )
                        }),
                )
            })
            .into_any_element()
    }

    fn unpaired_header(&mut self, count: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        let open = self.unpaired_open;
        div()
            .id("unpaired-toggle")
            .h(px(40.))
            .w_full()
            .px(px(28.))
            .flex()
            .items_center()
            .gap(px(8.))
            .cursor(CursorStyle::PointingHand)
            .bg(rgb(0x2A2A2C))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.unpaired_open = !this.unpaired_open;
                cx.notify();
            }))
            .child(icon(if open { CHEVRON_UP } else { CHEVRON_DOWN }, 14., MUTED))
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(MUTED))
                    .child("Unpaired"),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(MUTED))
                    .child(SharedString::from(count.to_string())),
            )
            .into_any_element()
    }

    fn pair_list(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let count = self.listed_pairs().len();
        let selected = self.selected_pair;
        let dragging_from = self
            .row_drag
            .as_ref()
            .filter(|drag| drag.moved)
            .map(|drag| drag.from);
        let drag_target = self
            .row_drag
            .as_ref()
            .filter(|drag| drag.moved)
            .map(|drag| drag.target);
        self.row_bounds.borrow_mut().clear();
        uniform_list(
            "library-scroll",
            count,
            cx.processor(move |this, range: Range<usize>, _, cx| {
                range
                    .map(|position| match this.listed_row(position) {
                        Some(row) => this.pair_row(row, selected, dragging_from, drag_target, cx),
                        None => div().h(px(PAIR_ROW_H)).w_full().into_any_element(),
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.library_scroll)
        .size_full()
        .into_any_element()
    }

    fn photo_list(
        &mut self,
        id: &'static str,
        selectable: bool,
        viewport_w: f32,
        sidebar: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let card_w = if selectable { SELECT_CARD_W } else { LIBRARY_CARD_W };
        let image_h = if selectable {
            SELECT_IMAGE_H
        } else {
            LIBRARY_IMAGE_H
        };
        let chrome = LIBRARY_CHROME + if sidebar { SIDEBAR_W } else { 0.0 };
        let columns = grid_columns(viewport_w, card_w, GRID_GAP, chrome);
        let count = self.library.unassigned().len();
        let rows = count.div_ceil(columns);
        let card_h = image_h + 44.0;
        let row_h = card_h + GRID_GAP;
        uniform_list(
            id,
            rows,
            cx.processor(move |this, range: Range<usize>, _, cx| {
                let photos: Vec<_> = this
                    .library
                    .unassigned()
                    .into_iter()
                    .map(|photo| (photo.id, photo.path.clone(), photo_label(photo)))
                    .collect();
                let before = this.library.draft_before;
                let after = this.library.draft_after;
                range
                    .map(|row| {
                        let start = row * columns;
                        let end = (start + columns).min(photos.len());
                        div()
                            .h(px(row_h))
                            .w_full()
                            .px(px(28.))
                            .pb(px(GRID_GAP))
                            .flex()
                            .gap(px(GRID_GAP))
                            .overflow_hidden()
                            .children(photos[start..end].iter().cloned().map(|(id, path, label)| {
                                let selected = before == Some(id) || after == Some(id);
                                let pick = (!selectable && this.arrange)
                                    .then(|| this.picked_photos.contains(&id));
                                image_card(
                                    id,
                                    path,
                                    label,
                                    card_w,
                                    image_h,
                                    selectable,
                                    selected,
                                    !selectable && this.arrange,
                                    pick,
                                    cx,
                                )
                            }))
                            .into_any_element()
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .size_full()
        .into_any_element()
    }

    fn images_heading(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let photos = self.library.photos().len();
        let unpaired = self.library.unassigned().len();
        let enabled = unpaired >= 2;
        div()
            .px(px(28.))
            .pt(px(22.))
            .pb(px(6.))
            .flex()
            .items_start()
            .justify_between()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(px(28.))
                            .font_weight(FontWeight::BOLD)
                            .child(SharedString::from(format!(
                                "{photos} {}",
                                plural(photos, "image", "images")
                            ))),
                    )
                    .child(
                        div().text_sm().text_color(rgb(MUTED)).child(SharedString::from(
                            format!("{unpaired} unpaired"),
                        )),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(quiet_pill(
                        "auto-pair-loose",
                        "Auto pair",
                        Some(LINK_ICON),
                        photos > 0,
                        cx.listener(|this, _: &ClickEvent, _, cx| this.open_auto_pair(cx)),
                    ))
                    .child(pill(
                        "open-create",
                        "Create pair",
                        Some(PAIR_ICON),
                        enabled,
                        cx.listener(|this, _: &ClickEvent, _, cx| this.open_create(cx)),
                    )),
            )
            .into_any_element()
    }

    fn create_screen(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let ready = self.create_slot(SlotSide::Before).is_some()
            && self.create_slot(SlotSide::After).is_some();
        let name = self.pair_name.clone();
        let viewport_w = px_f(window.viewport_size().width);
        div()
            .flex_1()
            .min_h(px(0.))
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_shrink_0()
                    .px(px(28.))
                    .pt(px(22.))
                    .flex()
                    .flex_col()
                    .gap(px(18.))
                    .child(
                        div()
                            .flex()
                            .gap(px(22.))
                            .child(self.editor_slot(SlotSide::Before, "Before", cx))
                            .child(self.editor_slot(SlotSide::After, "After", cx)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(
                                div().flex_1().child(self.input_box(
                                    "pair-name".into(),
                                    Field::PairName,
                                    name,
                                    "Name this pair",
                                    false,
                                    cx,
                                )),
                            )
                            .child(pill(
                                "commit-pair",
                                "Create pair",
                                Some(PAIR_ICON),
                                ready,
                                cx.listener(|this, _: &ClickEvent, _, cx| this.commit_pair(cx)),
                            )),
                    )
                    .child(div().h(px(1.)).bg(rgb(LINE)).my(px(4.)))
                    .child(
                        div()
                            .text_size(px(18.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Imported images"),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_hidden()
                    .child(self.photo_list("create-photos", true, viewport_w, false, cx)),
            )
            .into_any_element()
    }

    fn new_group_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let name = self.pair_name.clone();
        let selected = self.group_picks.len();
        let ready = selected > 0;
        let pair_ids: Vec<u64> = self.library.pairs.iter().map(|pair| pair.id).collect();
        let select_label = group_select_label(&pair_ids, &self.group_picks);
        let can_select = !pair_ids.is_empty();
        let count = pair_ids.len();
        let card_h = (px_f(window.viewport_size().height) - 120.0).clamp(440.0, 680.0);

        div()
            .id("group-backdrop")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .bg(rgb(0x000000).opacity(0.55))
            .flex()
            .items_center()
            .justify_center()
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.cancel_group(cx)))
            .child(
                div()
                    .id("group-modal")
                    .w(px(760.))
                    .h(px(card_h))
                    .flex()
                    .flex_col()
                    .rounded(px(18.))
                    .border_1()
                    .border_color(rgb(LINE))
                    .bg(rgb(0x161616))
                    .overflow_hidden()
                    .on_click(|_: &ClickEvent, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .px(px(22.))
                            .pt(px(18.))
                            .pb(px(8.))
                            .flex()
                            .items_start()
                            .justify_between()
                            .gap(px(16.))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .child(
                                        div()
                                            .text_size(px(22.))
                                            .font_weight(FontWeight::BOLD)
                                            .child("New group"),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(rgb(MUTED))
                                            .child("Choose pairs to include."),
                                    ),
                            )
                            .child(edit_text_button(
                                "group-select-all",
                                SharedString::from(select_label),
                                false,
                                can_select,
                                cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.toggle_group_select_all(cx);
                                }),
                            )),
                    )
                    .child(
                        div()
                            .px(px(22.))
                            .pt(px(12.))
                            .pb(px(14.))
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(div().text_sm().child("Name"))
                            .child(self.input_box(
                                "group-name".into(),
                                Field::GroupName,
                                name,
                                "Name",
                                true,
                                cx,
                            )),
                    )
                    .child(
                        uniform_list(
                            "group-picks",
                            count,
                            cx.processor(|this, range: Range<usize>, _, cx| {
                                range
                                    .map(|index| {
                                        let Some(pair) = this.library.pairs.get(index) else {
                                            return div().h(px(92.)).into_any_element();
                                        };
                                        let pair_id = pair.id;
                                        let title = pair.name.clone();
                                        let before_id = pair.before;
                                        let after_id = pair.after;
                                        let before = this
                                            .library
                                            .photo(before_id)
                                            .map(|photo| photo.path.clone())
                                            .unwrap_or_default();
                                        let after = this
                                            .library
                                            .photo(after_id)
                                            .map(|photo| photo.path.clone())
                                            .unwrap_or_default();
                                        let checked = this.group_picks.contains(&pair_id);
                                        div()
                                            .h(px(92.))
                                            .w_full()
                                            .px(px(22.))
                                            .pb(px(8.))
                                            .child(pair_check_row(
                                                pair_id, title, before, after, before_id, after_id,
                                                checked, cx,
                                            ))
                                            .into_any_element()
                                    })
                                    .collect::<Vec<_>>()
                            }),
                        )
                        .flex_1()
                        .min_h(px(0.)),
                    )
                    .child(
                        div()
                            .h(px(64.))
                            .px(px(22.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_between()
                            .border_t_1()
                            .border_color(rgb(LINE))
                            .child(
                                div().text_sm().text_color(rgb(MUTED)).child(SharedString::from(
                                    format!(
                                        "{selected} {} selected",
                                        plural(selected, "pair", "pairs"),
                                    ),
                                )),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(16.))
                                    .child(text_button(
                                        "cancel-group",
                                        "Cancel",
                                        cx.listener(|this, _: &ClickEvent, _, cx| {
                                            this.cancel_group(cx);
                                        }),
                                    ))
                                    .child(pill(
                                        "commit-group",
                                        "Create group",
                                        None,
                                        ready,
                                        cx.listener(|this, _: &ClickEvent, _, cx| {
                                            this.commit_group(cx);
                                        }),
                                    )),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn delete_overlay(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let count = self.picked_pairs.len() + self.picked_photos.len();
        let title = format!("Delete {} {}?", count, plural(count, "item", "items"));
        div()
            .id("delete-backdrop")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .bg(rgb(0x000000).opacity(0.55))
            .flex()
            .items_center()
            .justify_center()
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.cancel_delete(cx)))
            .child(
                div()
                    .id("delete-modal")
                    .w(px(440.))
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .p(px(22.))
                    .rounded(px(18.))
                    .border_1()
                    .border_color(rgb(LINE))
                    .bg(rgb(0x161616))
                    .on_click(|_: &ClickEvent, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .text_size(px(20.))
                            .font_weight(FontWeight::BOLD)
                            .child(SharedString::from(title)),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(MUTED))
                            .child("Selected pairs are removed with both photos. Selected unpaired photos are removed. Original files outside the library stay on disk."),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .items_center()
                            .gap(px(8.))
                            .pt(px(6.))
                            .child(text_button(
                                "cancel-delete",
                                "Cancel",
                                cx.listener(|this, _: &ClickEvent, _, cx| this.cancel_delete(cx)),
                            ))
                            .child(danger_pill(
                                "confirm-delete",
                                "Delete",
                                cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.commit_delete_picked(cx);
                                }),
                            )),
                    ),
            )
            .into_any_element()
    }

    fn input_box(
        &mut self,
        id: SharedString,
        field: Field,
        text: String,
        placeholder: &'static str,
        accent: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let focused = self.focused == Some(field);
        let caret = self.caret;
        let empty = text.is_empty();
        let border = if focused {
            if accent { BLUE } else { 0x5A5A5E }
        } else {
            LINE
        };
        div()
            .id(id)
            .w_full()
            .h(px(44.))
            .px(px(16.))
            .rounded(px(12.))
            .border_1()
            .border_color(rgb(border))
            .bg(rgb(FIELD))
            .flex()
            .items_center()
            .cursor(CursorStyle::IBeam)
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.focus_field(field);
                cx.notify();
            }))
            .child(if empty {
                div()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .when(focused, |this| this.child(caret_runs("", caret, true)))
                    .child(div().text_color(rgb(MUTED)).child(placeholder))
                    .into_any_element()
            } else {
                caret_runs(&text, caret, focused)
            })
            .into_any_element()
    }

    fn groups_button(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let count = self.viewer_groups.len();
        let label = if count == 0 {
            SharedString::from("Groups")
        } else {
            SharedString::from(format!("Groups · {count}"))
        };
        div()
            .id("groups-toggle")
            .h(px(32.))
            .px(px(12.))
            .rounded_full()
            .flex()
            .items_center()
            .gap(px(6.))
            .bg(rgb(0x2A2A2C))
            .text_sm()
            .cursor(CursorStyle::PointingHand)
            .hover(|style| style.bg(rgb(HOVER)))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.groups_menu = !this.groups_menu;
                if this.groups_menu {
                    this.fit_menu = false;
                    this.mode_menu = false;
                    this.scope_menu = false;
                }
                cx.notify();
            }))
            .child(label)
            .child(div().text_xs().text_color(rgb(MUTED)).child("▾"))
            .into_any_element()
    }

    fn groups_overlay(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let checked = self.viewer_groups.clone();
        let rows: Vec<_> = self
            .library
            .groups()
            .iter()
            .map(|group| {
                (
                    group.id,
                    group.name.clone(),
                    group.pair_count(),
                    checked.contains(&group.id),
                )
            })
            .collect();
        let visible = self.visible_indices().len();
        let empty = rows.is_empty();
        div()
            .id("groups-backdrop")
            .absolute()
            .inset_0()
            .top(px(HEADER))
            .occlude()
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.groups_menu = false;
                cx.notify();
            }))
            .child(
                div()
                    .id("groups-menu")
                    .absolute()
                    .top(px(8.))
                    .right(px(108.))
                    .w(px(268.))
                    .rounded(px(14.))
                    .border_1()
                    .border_color(rgb(LINE))
                    .bg(rgb(0x242424))
                    .py(px(8.))
                    .on_click(|_: &ClickEvent, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .absolute()
                            .top(px(-11.))
                            .right(px(46.))
                            .text_color(rgb(0x242424))
                            .child("▴"),
                    )
                    .child(
                        div()
                            .px(px(14.))
                            .pt(px(6.))
                            .pb(px(8.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Visible groups"),
                    )
                    .when(empty, |this| {
                        this.child(
                            div()
                                .px(px(14.))
                                .py(px(6.))
                                .text_sm()
                                .text_color(rgb(MUTED))
                                .child("No groups yet"),
                        )
                    })
                    .children(rows.into_iter().map(|(id, name, count, on)| {
                        viewer_group_row(id, name, count, on, cx)
                    }))
                    .child(div().mx(px(14.)).h(px(1.)).bg(rgb(LINE)).my(px(6.)))
                    .child(
                        div()
                            .px(px(14.))
                            .py(px(4.))
                            .text_sm()
                            .text_color(rgb(MUTED))
                            .child(SharedString::from(format!(
                                "{visible} {} visible",
                                plural(visible, "pair", "pairs"),
                            ))),
                    )
                    .child(
                        div()
                            .id("show-all-pairs")
                            .h(px(36.))
                            .px(px(14.))
                            .flex()
                            .items_center()
                            .font_weight(FontWeight::SEMIBOLD)
                            .cursor(CursorStyle::PointingHand)
                            .hover(|style| style.bg(rgb(0x2E2E32)))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.show_all_in_viewer(cx);
                                cx.stop_propagation();
                            }))
                            .child("Show all pairs"),
                    ),
            )
            .into_any_element()
    }

    fn viewer(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        if self.library.pairs.is_empty() {
            return self.empty_state(cx);
        }
        let visible = self.visible_indices();
        if visible.is_empty() {
            return div()
                .flex_1()
                .min_h(px(0.))
                .flex()
                .items_center()
                .justify_center()
                .text_color(rgb(MUTED))
                .child("No pairs in these groups")
                .into_any_element();
        }
        if !visible.contains(&self.focus_pair) {
            self.focus_pair = visible[0];
            self.selected_pair = self.focus_pair;
        }
        let viewport = window.viewport_size();
        let panel = if self.notes_open { NOTES_W } else { 0.0 };
        let content_w = (px_f(viewport.width) - VIEW_PAD * 2.0 - panel).max(1.0);
        let content_h = (px_f(viewport.height) - HEADER - FOOTER - VIEW_PAD * 2.0).max(1.0);
        let position = visible
            .iter()
            .position(|index| *index == self.focus_pair)
            .unwrap_or(0);
        let index = visible[position];
        let has_prev = position > 0;
        let has_next = position + 1 < visible.len();
        let (prev_h, main_h, next_h) = stack_heights(content_h, has_prev, has_next);
        let prev = has_prev.then(|| visible[position - 1]);
        let next = has_next.then(|| visible[position + 1]);

        let notes = self.notes_open.then(|| self.notes_panel(cx));
        div()
            .flex_1()
            .min_h(px(0.))
            .flex()
            .flex_row()
            .overflow_hidden()
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w(px(0.))
                    .h_full()
                    .flex()
                    .flex_col()
                    .px(px(VIEW_PAD))
                    .py(px(VIEW_PAD))
                    .gap(px(GAP))
                    .overflow_hidden()
                    .when(has_prev, |this| {
                        this.child(self.clipped(prev.unwrap(), content_w, main_h, prev_h, true, cx))
                    })
                    .child(self.comparison(index, content_w, main_h, true, cx))
                    .when(has_next, |this| {
                        this.child(self.clipped(next.unwrap(), content_w, main_h, next_h, false, cx))
                    })
                    .when(self.notes_open, |this| this.child(self.pin_button(cx))),
            )
            .children(notes)
            .into_any_element()
    }

    fn clipped(
        &mut self,
        index: usize,
        width: f32,
        full_h: f32,
        visible_h: f32,
        align_bottom: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let shift = if align_bottom { visible_h - full_h } else { 0.0 };
        let card = self.comparison(index, width, full_h, false, cx);
        div()
            .relative()
            .w(px(width))
            .h(px(visible_h))
            .overflow_hidden()
            .rounded(px(16.))
            .child(
                div()
                    .absolute()
                    .top(px(shift))
                    .left_0()
                    .w(px(width))
                    .h(px(full_h))
                    .child(card),
            )
            .into_any_element()
    }

    fn back_button(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        div()
            .id("back-viewer")
            .h(px(32.))
            .px(px(12.))
            .rounded_full()
            .flex()
            .items_center()
            .gap(px(6.))
            .bg(rgb(0x2A2A2C))
            .text_sm()
            .cursor(CursorStyle::PointingHand)
            .hover(|style| style.bg(rgb(HOVER)))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.show_library(cx)))
            .child(icon(CHEVRON_LEFT, 16., TEXT))
            .child("Back")
            .into_any_element()
    }

    fn search_button(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        div()
            .id("search-toggle")
            .w(px(32.))
            .h(px(32.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(8.))
            .cursor(CursorStyle::PointingHand)
            .hover(|style| style.bg(rgb(HOVER)))
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                this.find(&Find, window, cx);
            }))
            .child(
                svg()
                    .data(SEARCH_ICON)
                    .w(px(20.))
                    .h(px(20.))
                    .text_color(rgb(if self.search_open { TEXT } else { MUTED })),
            )
            .into_any_element()
    }

    fn notes_button(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let count = self
            .focus_pair_id()
            .map(|id| self.library.note_count(id))
            .unwrap_or(0);
        let label = if count == 0 {
            SharedString::from("Notes")
        } else {
            SharedString::from(format!("Notes · {count}"))
        };
        div()
            .id("notes-toggle")
            .h(px(32.))
            .px(px(12.))
            .rounded_full()
            .flex()
            .items_center()
            .bg(rgb(if self.notes_open { 0x343438 } else { 0x2A2A2C }))
            .text_sm()
            .cursor(CursorStyle::PointingHand)
            .hover(|style| style.bg(rgb(if self.notes_open { 0x404044 } else { HOVER })))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.notes_open = !this.notes_open;
                if !this.notes_open {
                    this.cancel_placement_silent();
                }
                this.close_menus();
                cx.notify();
            }))
            .child(label)
            .into_any_element()
    }

    fn pin_button(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let placing = self.placing;
        div()
            .id("add-pin")
            .absolute()
            .bottom(px(8.))
            .right(px(8.))
            .h(px(32.))
            .px(px(12.))
            .rounded_full()
            .border_1()
            .border_color(rgb(LINE))
            .bg(rgb(0x1C1C1E))
            .flex()
            .items_center()
            .text_sm()
            .cursor(CursorStyle::PointingHand)
            .hover(|style| style.bg(rgb(HOVER)))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                if placing {
                    this.cancel_placement(cx);
                } else {
                    this.begin_placement(cx);
                }
            }))
            .child(if placing { "Cancel pin" } else { "Add pin" })
            .into_any_element()
    }

    fn step_buttons(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        div()
            .flex()
            .items_center()
            .gap(px(4.))
            .child(self.step_button("step-prev", CHEVRON_LEFT, -1, cx))
            .child(self.step_button("step-next", CHEVRON_RIGHT, 1, cx))
            .into_any_element()
    }

    fn step_button(
        &mut self,
        id: &'static str,
        glyph: &'static [u8],
        delta: i32,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        div()
            .id(id)
            .w(px(36.))
            .h(px(32.))
            .rounded(px(8.))
            .border_1()
            .border_color(rgb(LINE))
            .flex()
            .items_center()
            .justify_center()
            .cursor(CursorStyle::PointingHand)
            .hover(|style| style.bg(rgb(HOVER)))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.focused = None;
                this.step_pairs(delta, cx);
            }))
            .child(icon(glyph, 18., TEXT))
            .into_any_element()
    }

    fn viewer_controls(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let overlay = self.compare_mode == CompareMode::Overlay;
        div()
            .flex()
            .items_center()
            .gap(px(14.))
            .child(self.mode_control(cx))
            .when(overlay, |this| this.child(self.blend_slider(cx)))
            .child(self.fit_control(cx))
            .into_any_element()
    }

    fn mode_control(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let label = match self.compare_mode {
            CompareMode::Wipe => "Wipe",
            CompareMode::Overlay => "Overlay",
        };
        let open = self.mode_menu;
        let wipe = self.compare_mode == CompareMode::Wipe;
        div()
            .relative()
            .child(
                div()
                    .id("mode-toggle")
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .cursor(CursorStyle::PointingHand)
                    .text_color(rgb(TEXT))
                    .hover(|style| style.opacity(0.72))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.mode_menu = !this.mode_menu;
                        if this.mode_menu {
                            this.fit_menu = false;
                            this.groups_menu = false;
                        }
                        cx.notify();
                    }))
                    .child(label)
                    .child(div().text_xs().text_color(rgb(MUTED)).child("▾")),
            )
            .when(open, |this| {
                this.child(
                    div()
                        .absolute()
                        .bottom(px(28.))
                        .left(px(-8.))
                        .w(px(148.))
                        .rounded(px(12.))
                        .border_1()
                        .border_color(rgb(LINE))
                        .bg(rgb(MENU))
                        .py(px(4.))
                        .id("mode-menu")
                        .on_click(|_: &ClickEvent, _, cx| cx.stop_propagation())
                        .child(mode_option("Wipe", wipe, CompareMode::Wipe, cx))
                        .child(mode_option("Overlay", !wipe, CompareMode::Overlay, cx)),
                )
            })
            .into_any_element()
    }

    fn blend_slider(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let blend = self.blend.clamp(0.0, 1.0);
        let percent = (blend * 100.0).round() as i32;
        let dragging = self.blend_drag;
        let frame = self.blend_frame.clone();
        let entity = cx.weak_entity();
        let width = 148.0;
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .text_sm()
            .text_color(rgb(MUTED))
            .child("Before")
            .child(
                div()
                    .id("blend-slider")
                    .relative()
                    .w(px(width))
                    .h(px(22.))
                    .cursor(CursorStyle::ResizeLeftRight)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, _, cx| {
                            this.begin_blend(event.position, cx);
                            cx.stop_propagation();
                        }),
                    )
                    .child(
                        canvas(
                            |bounds, _, _| bounds,
                            move |bounds, _, window, _| {
                                frame.set(Some(bounds));
                                if !dragging {
                                    return;
                                }
                                let move_entity = entity.clone();
                                window.on_mouse_event(
                                    move |event: &MouseMoveEvent, phase, _, cx| {
                                        if phase != DispatchPhase::Bubble {
                                            return;
                                        }
                                        let _ = move_entity.update(cx, |this, cx| {
                                            this.track_move(event.position, cx);
                                        });
                                    },
                                );
                                let up_entity = entity.clone();
                                window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                                    if phase != DispatchPhase::Bubble
                                        || event.button != MouseButton::Left
                                    {
                                        return;
                                    }
                                    let _ = up_entity.update(cx, |this, cx| this.track_up(cx));
                                });
                            },
                        )
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full(),
                    )
                    .child(
                        div()
                            .absolute()
                            .top(px(9.))
                            .left_0()
                            .w(px(width))
                            .h(px(4.))
                            .rounded_full()
                            .bg(rgb(0x3A3A3C)),
                    )
                    .child(
                        div()
                            .absolute()
                            .top(px(9.))
                            .left_0()
                            .w(px(blend * width))
                            .h(px(4.))
                            .rounded_full()
                            .bg(rgb(BLUE)),
                    )
                    .child(
                        div()
                            .absolute()
                            .top(px(4.))
                            .left(px((blend * width - 7.0).clamp(0.0, width - 14.0)))
                            .size(px(14.))
                            .rounded_full()
                            .bg(rgb(0xFFFFFF)),
                    ),
            )
            .child(
                div()
                    .w(px(40.))
                    .text_color(rgb(TEXT))
                    .child(SharedString::from(format!("{percent}%"))),
            )
            .child("After")
            .into_any_element()
    }

    fn edit_screen(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let name = self.pair_name.clone();
        div()
            .flex_1()
            .min_h(px(0.))
            .id("edit-scroll")
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .px(px(28.))
            .pt(px(22.))
            .pb(px(28.))
            .gap(px(18.))
            .child(self.input_box(
                "edit-name".into(),
                Field::PairName,
                name,
                "Name this pair",
                false,
                cx,
            ))
            .child(
                div()
                    .flex()
                    .gap(px(18.))
                    .child(self.editor_slot(SlotSide::Before, "Before", cx))
                    .child(self.editor_slot(SlotSide::After, "After", cx)),
            )
            .into_any_element()
    }

    fn editor_slot(
        &mut self,
        side: SlotSide,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let selected = self.slot == side;
        let photo = if self.screen == Screen::Edit {
            self.edit_slot(side)
        } else {
            self.create_slot(side)
        };
        let image_h = if self.screen == Screen::Edit {
            360.0
        } else {
            240.0
        };
        let slot_id = match side {
            SlotSide::Before => "slot-before",
            SlotSide::After => "slot-after",
        };
        div()
            .flex_1()
            .min_w(px(0.))
            .id(slot_id)
            .rounded(px(18.))
            .border(px(if selected { 2. } else { 1. }))
            .border_color(rgb(if selected { BLUE } else { LINE }))
            .bg(rgb(CARD))
            .flex()
            .flex_col()
            .cursor(CursorStyle::PointingHand)
            .hover(|style| style.bg(rgb(0x222224)))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.select_slot(side, cx);
                cx.stop_propagation();
            }))
            .child(
                div()
                    .px(px(16.))
                    .pt(px(14.))
                    .pb(px(8.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .font_weight(FontWeight::BOLD)
                            .text_size(px(18.))
                            .child(label),
                    )
                    .when(selected, |this| {
                        this.child(
                            div()
                                .id(match side {
                                    SlotSide::Before => "paste-before",
                                    SlotSide::After => "paste-after",
                                })
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .text_sm()
                                .text_color(rgb(MUTED))
                                .cursor(CursorStyle::PointingHand)
                                .hover(|style| style.text_color(rgb(TEXT)))
                                .on_click(cx.listener(
                                    move |this, _: &ClickEvent, window, cx| {
                                        this.slot = side;
                                        this.focused = None;
                                        this.paste_image(&PasteImage, window, cx);
                                        cx.stop_propagation();
                                    },
                                ))
                                .child("Paste")
                                .child(
                                    div()
                                        .px(px(6.))
                                        .py(px(2.))
                                        .rounded(px(6.))
                                        .bg(rgb(0x2C2C2E))
                                        .text_xs()
                                        .text_color(rgb(TEXT))
                                        .child(SharedString::from(shortcut("V"))),
                                ),
                        )
                    }),
            )
            .child(match photo {
                Some((path, caption)) => div()
                    .px(px(12.))
                    .pb(px(12.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .h(px(image_h))
                            .rounded(px(12.))
                            .overflow_hidden()
                            .bg(rgb(0x101010))
                            .child(
                                img(path)
                                    .id(SharedString::from(format!("slot-img-{slot_id}")))
                                    .size_full()
                                    .object_fit(ObjectFit::Contain),
                            ),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(MUTED))
                            .child(SharedString::from(caption)),
                    )
                    .into_any_element(),
                None => div()
                    .mx(px(12.))
                    .mb(px(12.))
                    .h(px(image_h))
                    .rounded(px(12.))
                    .border_1()
                    .border_dashed()
                    .border_color(rgb(LINE))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_sm()
                    .text_color(rgb(MUTED))
                    .child("Paste or choose an image")
                    .into_any_element(),
            })
            .into_any_element()
    }

    fn notes_panel(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(pair_id) = self.focus_pair_id() else {
            return div().w(px(NOTES_W)).into_any_element();
        };
        let (title, notes, pins) = {
            let pair = self.library.pair_by_id(pair_id).unwrap();
            let notes: Vec<(u64, String)> = pair
                .notes
                .iter()
                .map(|note| (note.id, note.text.clone()))
                .collect();
            let pins: Vec<(usize, u64, String, String)> = pair
                .pins
                .iter()
                .enumerate()
                .map(|(index, pin)| (index + 1, pin.id, pin.title.clone(), pin.text.clone()))
                .collect();
            (pair.name.clone(), notes, pins)
        };
        let selected = self.selected_pin;
        div()
            .w(px(NOTES_W))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .bg(rgb(0x161616))
            .border_l_1()
            .border_color(rgb(LINE))
            .child(
                div()
                    .px(px(16.))
                    .pt(px(16.))
                    .pb(px(10.))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(20.))
                                    .font_weight(FontWeight::BOLD)
                                    .child("Notes"),
                            )
                            .child(
                                div()
                                    .id("close-notes")
                                    .cursor(CursorStyle::PointingHand)
                                    .text_color(rgb(MUTED))
                                    .text_size(px(18.))
                                    .hover(|style| style.text_color(rgb(TEXT)))
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.notes_open = false;
                                        this.cancel_placement_silent();
                                        cx.notify();
                                    }))
                                    .child("×"),
                            ),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(MUTED))
                            .child(SharedString::from(title)),
                    ),
            )
            .child(
                div()
                    .id("notes-scroll")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .px(px(16.))
                    .pb(px(18.))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .when(!notes.is_empty(), |this| {
                        this.child(div().text_xs().text_color(rgb(MUTED)).child("Pair note"))
                    })
                    .children(
                        notes
                            .into_iter()
                            .map(|(id, text)| self.note_card(id, text, cx)),
                    )
                    .children(pins.into_iter().map(|(number, id, title, body)| {
                        self.pin_card(number, id, title, body, selected == Some(id), cx)
                    }))
                    .child(
                        div()
                            .id("add-note")
                            .h(px(32.))
                            .flex()
                            .items_center()
                            .text_sm()
                            .text_color(rgb(MUTED))
                            .cursor(CursorStyle::PointingHand)
                            .gap(px(6.))
                            .rounded(px(8.))
                            .px(px(8.))
                            .hover(|style| style.bg(rgb(0x2A2A2C)).text_color(rgb(TEXT)))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.add_pair_note(cx);
                            }))
                            .child(icon(PLUS_ICON, 14., MUTED))
                            .child("Add note"),
                    )
                    .child(
                        div()
                            .id("edit-from-notes")
                            .h(px(32.))
                            .px(px(8.))
                            .rounded(px(8.))
                            .flex()
                            .items_center()
                            .text_sm()
                            .cursor(CursorStyle::PointingHand)
                            .hover(|style| style.bg(rgb(0x2A2A2C)))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.open_edit(pair_id, Screen::Viewer, cx);
                            }))
                            .child("Edit pair"),
                    ),
            )
            .into_any_element()
    }

    fn note_card(&mut self, id: u64, text: String, cx: &mut Context<Self>) -> gpui::AnyElement {
        div()
            .rounded(px(12.))
            .border_1()
            .border_color(rgb(LINE))
            .bg(rgb(0x1C1C1E))
            .p(px(8.))
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(self.text_field(
                SharedString::from(format!("note-{id}")),
                Field::Note(id),
                text,
                "Write a note",
                48.0,
                cx,
            ))
            .child(
                div()
                    .id(SharedString::from(format!("delete-note-{id}")))
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .cursor(CursorStyle::PointingHand)
                    .hover(|style| style.text_color(rgb(RED)))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.delete_pair_note(id, cx);
                        cx.stop_propagation();
                    }))
                    .child("Delete"),
            )
            .into_any_element()
    }

    fn pin_card(
        &mut self,
        number: usize,
        id: u64,
        title: String,
        body: String,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        div()
            .id(SharedString::from(format!("pin-card-{id}")))
            .rounded(px(12.))
            .border(px(if selected { 2. } else { 1. }))
            .border_color(rgb(if selected { BLUE } else { LINE }))
            .bg(rgb(if selected { 0x1A2744 } else { 0x1C1C1E }))
            .p(px(8.))
            .flex()
            .flex_col()
            .gap(px(6.))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.selected_pin = Some(id);
                this.focus_field(Field::PinTitle(id));
                cx.notify();
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(number_badge(number, selected))
                    .child(self.text_field(
                        SharedString::from(format!("pin-title-{id}")),
                        Field::PinTitle(id),
                        title,
                        "Title",
                        32.0,
                        cx,
                    )),
            )
            .child(self.text_field(
                SharedString::from(format!("pin-body-{id}")),
                Field::PinBody(id),
                body,
                "Describe this mark",
                48.0,
                cx,
            ))
            .child(
                div()
                    .id(SharedString::from(format!("delete-pin-{id}")))
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .cursor(CursorStyle::PointingHand)
                    .hover(|style| style.text_color(rgb(RED)))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.delete_selected_pin(id, cx);
                        cx.stop_propagation();
                    }))
                    .child("Delete"),
            )
            .into_any_element()
    }

    fn text_field(
        &mut self,
        id: SharedString,
        field: Field,
        text: String,
        placeholder: &'static str,
        min_h: f32,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let focused = self.focused == Some(field);
        let caret = self.caret;
        let empty = text.is_empty();
        div()
            .id(id)
            .w_full()
            .min_h(px(min_h))
            .px(px(8.))
            .py(px(6.))
            .rounded(px(8.))
            .bg(rgb(FIELD))
            .cursor(CursorStyle::IBeam)
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.focus_field(field);
                cx.stop_propagation();
                cx.notify();
            }))
            .child(if empty && !focused {
                div()
                    .text_sm()
                    .text_color(rgb(MUTED))
                    .child(placeholder)
                    .into_any_element()
            } else {
                div()
                    .text_sm()
                    .w_full()
                    .child(caret_block(&text, caret, focused))
                    .into_any_element()
            })
            .into_any_element()
    }

    fn pin_mark(
        &mut self,
        pin: &Pin,
        number: usize,
        frame_w: f32,
        frame_h: f32,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let id = pin.id;
        let selected = self.selected_pin == Some(id);
        let point = pin.w < 0.012 && pin.h < 0.012;
        if point {
            return self.pin_badge(
                id,
                number,
                selected,
                pin.x * frame_w - 11.0,
                pin.y * frame_h - 11.0,
                cx,
            );
        }
        let left = pin.x * frame_w;
        let top = pin.y * frame_h;
        let width = (pin.w * frame_w).max(8.0);
        let height = (pin.h * frame_h).max(8.0);
        let color = if selected { BLUE } else { 0xFFFFFF };
        div()
            .id(SharedString::from(format!("pin-rect-{id}")))
            .absolute()
            .left(px(left))
            .top(px(top))
            .w(px(width))
            .h(px(height))
            .border(px(2.))
            .border_color(rgb(color))
            .cursor(CursorStyle::PointingHand)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.selected_pin = Some(id);
                    this.focus_field(Field::PinTitle(id));
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .child(self.pin_badge(id, number, selected, width - 11.0, -11.0, cx))
            .when(selected, |this| {
                this.child(self.pin_handle(id, width, height, cx))
            })
            .into_any_element()
    }

    fn pin_badge(
        &mut self,
        id: u64,
        number: usize,
        selected: bool,
        left: f32,
        top: f32,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        div()
            .id(SharedString::from(format!("pin-badge-{id}")))
            .absolute()
            .left(px(left))
            .top(px(top))
            .size(px(22.))
            .rounded_full()
            .bg(rgb(if selected { BLUE } else { 0x3A3A3C }))
            .border_1()
            .border_color(rgb(0xFFFFFF))
            .flex()
            .items_center()
            .justify_center()
            .text_xs()
            .font_weight(FontWeight::BOLD)
            .text_color(rgb(0xFFFFFF))
            .cursor(CursorStyle::OpenHand)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    this.begin_pin_move(id, event.position, cx);
                    cx.stop_propagation();
                }),
            )
            .child(SharedString::from(number.to_string()))
            .into_any_element()
    }

    fn pin_handle(
        &mut self,
        id: u64,
        width: f32,
        height: f32,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        div()
            .id(SharedString::from(format!("pin-size-{id}")))
            .absolute()
            .left(px((width - 8.0).max(0.0)))
            .top(px((height - 8.0).max(0.0)))
            .size(px(10.))
            .rounded(px(2.))
            .bg(rgb(BLUE))
            .cursor(CursorStyle::ResizeUpLeftDownRight)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.begin_pin_resize(id, cx);
                    cx.stop_propagation();
                }),
            )
            .into_any_element()
    }

    fn pin_catch(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        div()
            .id("pin-catch")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .cursor(CursorStyle::Crosshair)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    this.begin_pin(event.position, cx);
                    cx.stop_propagation();
                }),
            )
            .into_any_element()
    }

    fn search_overlay(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let hits = self.search_hit_indexes();
        let query = self.search_query.clone();
        let scope = self.search_scope;
        let menu = self.scope_menu;
        let count = hits.len();
        let selected_slot = if count == 0 {
            0
        } else {
            self.search_selected.min(count - 1)
        };
        let open_outside = hits
            .get(selected_slot)
            .is_some_and(|index| self.outside_visible(*index));
        let open_label = if open_outside {
            "↩ Open in All pairs"
        } else {
            "↩ Open"
        };
        div()
            .id("search-backdrop")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.close_search(cx)))
            .child(
                div()
                    .absolute()
                    .top(px(HEADER + 8.0))
                    .left_0()
                    .right_0()
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .id("search-popover")
                            .w(px(640.))
                            .rounded(px(16.))
                            .border_1()
                            .border_color(rgb(BLUE))
                            .bg(rgb(0x1C1C1E))
                            .on_click(|_: &ClickEvent, _, cx| cx.stop_propagation())
                            .flex()
                            .flex_col()
                            .child(
                                div().px(px(12.)).pt(px(12.)).child(self.input_box(
                                    "search-query".into(),
                                    Field::Search,
                                    query.clone(),
                                    "Search pairs",
                                    true,
                                    cx,
                                )),
                            )
                            .child(
                                div()
                                    .px(px(14.))
                                    .pt(px(10.))
                                    .pb(px(6.))
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .text_sm()
                                    .text_color(rgb(MUTED))
                                    .child("Search in")
                                    .child(
                                        div()
                                            .id("search-scope")
                                            .relative()
                                            .h(px(28.))
                                            .px(px(10.))
                                            .rounded(px(8.))
                                            .bg(rgb(0x2A2A2C))
                                            .flex()
                                            .items_center()
                                            .gap(px(6.))
                                            .text_color(rgb(TEXT))
                                            .cursor(CursorStyle::PointingHand)
                                            .hover(|style| style.bg(rgb(HOVER)))
                                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                                this.scope_menu = !this.scope_menu;
                                                cx.stop_propagation();
                                                cx.notify();
                                            }))
                                            .child(scope_name(scope))
                                            .child("▾")
                                            .when(menu, |this| {
                                                this.child(
                                                    div()
                                                        .absolute()
                                                        .top(px(32.))
                                                        .left_0()
                                                        .w(px(180.))
                                                        .rounded(px(10.))
                                                        .border_1()
                                                        .border_color(rgb(LINE))
                                                        .bg(rgb(MENU))
                                                        .py(px(4.))
                                                        .child(scope_option(
                                                            "Current selection",
                                                            SearchScope::Current,
                                                            scope == SearchScope::Current,
                                                            cx,
                                                        ))
                                                        .child(scope_option(
                                                            "All pairs",
                                                            SearchScope::All,
                                                            scope == SearchScope::All,
                                                            cx,
                                                        )),
                                                )
                                            }),
                                    ),
                            )
                            .child(
                                div()
                                    .h(px(280.))
                                    .overflow_hidden()
                                    .when(count == 0, |this| {
                                        this.child(
                                            div()
                                                .h(px(120.))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .text_sm()
                                                .text_color(rgb(MUTED))
                                                .child("No matches"),
                                        )
                                    })
                                    .when(count > 0, |this| {
                                        this.child(
                                            uniform_list(
                                                "search-results",
                                                count,
                                                cx.processor(
                                                    move |this, range: Range<usize>, _, cx| {
                                                        let query = this.search_query.clone();
                                                        let hits = this.search_hit_indexes();
                                                        let selected = this
                                                            .search_selected
                                                            .min(hits.len().saturating_sub(1));
                                                        range
                                                            .map(|slot| {
                                                                this.search_row(
                                                                    slot, &hits, &query, selected,
                                                                    cx,
                                                                )
                                                            })
                                                            .collect::<Vec<_>>()
                                                    },
                                                ),
                                            )
                                            .h(px(280.))
                                            .px(px(8.)),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .px(px(14.))
                                    .py(px(10.))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .text_xs()
                                    .text_color(rgb(MUTED))
                                    .border_t_1()
                                    .border_color(rgb(LINE))
                                    .child(SharedString::from(format!(
                                        "{count} {} found",
                                        plural(count, "pair", "pairs")
                                    )))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(12.))
                                            .child("↑ ↓ Select")
                                            .child(open_label)
                                            .child("Esc Close"),
                                    ),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn search_row(
        &mut self,
        slot: usize,
        hits: &[usize],
        query: &str,
        selected: usize,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(index) = hits.get(slot).copied() else {
            return div().h(px(72.)).into_any_element();
        };
        let Some(pair) = self.library.pairs.get(index) else {
            return div().h(px(72.)).into_any_element();
        };
        let before = self.library.photo(pair.before);
        let after = self.library.photo(pair.after);
        let hit = SearchHit {
            slot,
            title: pair.name.clone(),
            before_name: self.filename(pair.before),
            after_name: self.filename(pair.after),
            groups: self.library.group_label(pair.id),
            before: before.map(|photo| photo.path.clone()).unwrap_or_default(),
            after: after.map(|photo| photo.path.clone()).unwrap_or_default(),
            before_id: pair.before,
            after_id: pair.after,
        };
        self.search_hit(hit, query.to_string(), slot == selected, cx)
    }

    fn search_hit(
        &mut self,
        hit: SearchHit,
        query: String,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let slot = hit.slot;
        div()
            .id(SharedString::from(format!("search-hit-{slot}")))
            .h(px(72.))
            .w_full()
            .px(px(10.))
            .py(px(8.))
            .rounded(px(12.))
            .flex()
            .items_center()
            .gap(px(12.))
            .cursor(CursorStyle::PointingHand)
            .bg(rgb(if selected { 0x243044 } else { 0x000000 }))
            .hover(|style| style.bg(rgb(if selected { 0x2C3C56 } else { 0x1C1C1E })))
            .border_1()
            .border_color(rgb(if selected { BLUE } else { 0x000000 }))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.search_selected = slot;
                this.open_search_result(cx);
                cx.stop_propagation();
            }))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(marked_text(&hit.title, &query))
                    .child(
                        div()
                            .flex()
                            .text_sm()
                            .text_color(rgb(MUTED))
                            .child(marked_text(&hit.before_name, &query))
                            .child(" · ")
                            .child(marked_text(&hit.after_name, &query)),
                    )
                    .when(!hit.groups.is_empty(), |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(rgb(MUTED))
                                .child(SharedString::from(hit.groups)),
                        )
                    }),
            )
            .child(thumb(hit.before_id, hit.before, 72.0, 48.0))
            .child(thumb(hit.after_id, hit.after, 72.0, 48.0))
            .into_any_element()
    }

    fn auto_overlay(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let proposals = self.proposals();
        let pool_empty = self.library.unassigned().is_empty();
        let ready = proposals.as_ref().map(|list| list.iter().filter(|item| item.is_ready()).count()).unwrap_or(0);
        let matched = ready * 2;
        let before_pattern = self.before_pattern.clone();
        let after_pattern = self.after_pattern.clone();
        let line_count = proposals.as_ref().map(|list| list.len()).unwrap_or(0);
        let error = proposals.as_ref().err().copied();
        let show_lines = error.is_none() && line_count > 0;
        let empty_copy = if pool_empty {
            "All images are already paired."
        } else {
            "No numbered matches yet."
        };
        div()
            .id("auto-backdrop")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .bg(rgb(0x000000).opacity(0.55))
            .flex()
            .items_center()
            .justify_center()
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.auto_pair = false;
                this.focused = None;
                cx.notify();
            }))
            .child(
                div()
                    .id("auto-modal")
                    .w(px(760.))
                    .rounded(px(18.))
                    .border_1()
                    .border_color(rgb(LINE))
                    .bg(rgb(0x1A1A1A))
                    .p(px(18.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .on_click(|_: &ClickEvent, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(22.))
                                    .font_weight(FontWeight::BOLD)
                                    .child("Auto pair"),
                            )
                            .child(
                                div()
                                    .id("close-auto")
                                    .cursor(CursorStyle::PointingHand)
                                    .text_color(rgb(MUTED))
                                    .text_size(px(18.))
                                    .hover(|style| style.text_color(rgb(TEXT)))
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.auto_pair = false;
                                        this.focused = None;
                                        cx.notify();
                                    }))
                                    .child("×"),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .justify_between()
                            .child(fake_select("Match in", "Filename"))
                            .child(fake_select("Rule", "Numbered pattern")),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(12.))
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.))
                                    .child(div().text_sm().text_color(rgb(MUTED)).child("Before pattern"))
                                    .child(self.input_box(
                                        "before-pattern".into(),
                                        Field::BeforePattern,
                                        before_pattern,
                                        "before_{n}",
                                        false,
                                        cx,
                                    )),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.))
                                    .child(div().text_sm().text_color(rgb(MUTED)).child("After pattern"))
                                    .child(self.input_box(
                                        "after-pattern".into(),
                                        Field::AfterPattern,
                                        after_pattern,
                                        "after_{n}",
                                        false,
                                        cx,
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(MUTED))
                            .child("Match the same number on both sides."),
                    )
                    .when(error.is_some(), |this| {
                        this.child(
                            div()
                                .text_sm()
                                .text_color(rgb(0xFFB020))
                                .child(error.unwrap_or("")),
                        )
                    })
                    .child(
                        div()
                            .h(px(300.))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(rgb(LINE))
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            .when(show_lines, |this| {
                                this.child(
                                    div()
                                        .px(px(12.))
                                        .h(px(32.))
                                        .flex_shrink_0()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.))
                                        .text_xs()
                                        .text_color(rgb(MUTED))
                                        .child(div().w(px(64.)).flex_shrink_0().child("Match"))
                                        .child(div().flex_1().min_w(px(0.)).child("Before"))
                                        .child(div().flex_1().min_w(px(0.)).child("After")),
                                )
                            })
                            .when(show_lines, |this| {
                                this.child(
                                    uniform_list(
                                        "auto-rows",
                                        line_count,
                                        cx.processor(|this, range: Range<usize>, _, _cx| {
                                            let proposals = this.proposals().unwrap_or_default();
                                            range
                                                .map(|index| match proposals.get(index) {
                                                    Some(proposal) => this.auto_line(index, proposal),
                                                    None => div().h(px(64.)).into_any_element(),
                                                })
                                                .collect::<Vec<_>>()
                                        }),
                                    )
                                    .flex_1()
                                    .min_h(px(0.)),
                                )
                            })
                            .when(!show_lines, |this| {
                                this.child(
                                    div()
                                        .flex_1()
                                        .min_h(px(0.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_sm()
                                        .text_color(rgb(MUTED))
                                        .child(if error.is_some() {
                                            "Fix the pattern to preview matches."
                                        } else {
                                            empty_copy
                                        }),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .pt(px(4.))
                            .child(
                                div().text_sm().text_color(rgb(MUTED)).child(SharedString::from(
                                    format!(
                                        "{ready} {} found · {matched} {} matched",
                                        plural(ready, "pair", "pairs"),
                                        plural(matched, "image", "images"),
                                    ),
                                )),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(14.))
                                    .child(text_button(
                                        "cancel-auto",
                                        "Cancel",
                                        cx.listener(|this, _: &ClickEvent, _, cx| {
                                            this.auto_pair = false;
                                            this.focused = None;
                                            cx.notify();
                                        }),
                                    ))
                                    .child(labeled_pill(
                                        "create-auto",
                                        create_label(ready),
                                        ready > 0,
                                        cx.listener(|this, _: &ClickEvent, _, cx| {
                                            this.commit_auto_pair(cx);
                                        }),
                                    )),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn auto_line(&self, index: usize, proposal: &Proposal) -> gpui::AnyElement {
        let (number, before, after) = match proposal {
            Proposal::Ready {
                number,
                before,
                after,
                ..
            } => (
                number.to_string(),
                auto_cell_file(self.auto_file(*before)),
                auto_cell_file(self.auto_file(*after)),
            ),
            Proposal::Missing {
                number,
                before,
                after,
                ..
            } => (
                number.to_string(),
                auto_cell_file(before.and_then(|id| self.auto_file(id))),
                auto_cell_file(after.and_then(|id| self.auto_file(id))),
            ),
            Proposal::Ambiguous { number, .. } => (
                number.to_string(),
                auto_cell_note("Several files share this number."),
                auto_cell_empty(),
            ),
            Proposal::Conflict { photo } => (
                "—".to_string(),
                auto_cell_file(self.auto_file(*photo)),
                auto_cell_note("Matches both patterns."),
            ),
        };
        div()
            .id(SharedString::from(format!("proposal-{index}")))
            .h(px(64.))
            .px(px(12.))
            .flex()
            .items_center()
            .gap(px(8.))
            .border_b_1()
            .border_color(rgb(LINE))
            .child(
                div()
                    .w(px(64.))
                    .flex_shrink_0()
                    .text_sm()
                    .child(SharedString::from(number)),
            )
            .child(div().flex_1().min_w(px(0.)).overflow_hidden().child(before))
            .child(div().flex_1().min_w(px(0.)).overflow_hidden().child(after))
            .into_any_element()
    }

    fn auto_file(&self, id: u64) -> Option<(u64, PathBuf, String)> {
        let photo = self.library.photo(id)?;
        Some((photo.id, photo.path.clone(), photo_label(photo)))
    }

    fn pair_row(
        &mut self,
        row: PairRow,
        selected_index: usize,
        dragging_from: Option<usize>,
        drag_target: Option<usize>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let position = row.position;
        let index = row.index;
        let pair_id = row.pair_id;
        let name = row.name.clone();
        let label = row.label.clone();
        let before = row.before.clone();
        let after = row.after.clone();
        let before_id = row.before_id;
        let after_id = row.after_id;
        let placeholder = dragging_from == Some(position);
        let line_top =
            drag_target == Some(position) && dragging_from.is_some_and(|from| position <= from);
        let line_bottom =
            drag_target == Some(position) && dragging_from.is_some_and(|from| position > from);
        let selected = selected_index == index && !placeholder;
        let arrange = self.arrange;
        let lead = if arrange {
            PAIR_PICK_LEFT + PAIR_PICK_SIZE + PAIR_PICK_GAP
        } else {
            28.0
        };
        let picked = self.picked_pairs.contains(&pair_id);
        let drag_row = row;
        let probe = row_probe(self.row_bounds.clone(), position);
        div()
            .id(SharedString::from(format!("pair-row-{pair_id}")))
            .relative()
            .h(px(PAIR_ROW_H))
            .w_full()
            .pb(px(10.))
            .when(line_top, |this| this.child(insert_line(true)))
            .when(line_bottom, |this| this.child(insert_line(false)))
            .when(placeholder, |this| {
                this.child(
                    div()
                        .h(px(84.))
                        .w_full()
                        .pl(px(lead))
                        .pr(px(28.))
                        .child(
                            div()
                                .h_full()
                                .w_full()
                                .rounded(px(16.))
                                .border_1()
                                .border_dashed()
                                .border_color(rgb(LINE))
                                .opacity(0.4),
                        ),
                )
            })
            .when(!placeholder, |this| {
                this.child(
                    div()
                        .w_full()
                        .pl(px(lead))
                        .pr(px(28.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .relative()
                        .child(probe)
                        .child(
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .p(px(10.))
                        .rounded(px(16.))
                        .bg(rgb(if selected { 0x242424 } else { CARD }))
                        .border_1()
                        .border_color(rgb(LINE))
                        .cursor(CursorStyle::PointingHand)
                        .hover(|style| style.bg(rgb(if selected { 0x2E2E32 } else { 0x222224 })))
                        .id(SharedString::from(format!("pair-open-{pair_id}")))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            if this.block_row_open {
                                this.block_row_open = false;
                                cx.stop_propagation();
                                return;
                            }
                            if this.arrange {
                                this.toggle_picked_pair(pair_id, cx);
                                cx.stop_propagation();
                                return;
                            }
                            this.selected_pair = index;
                            this.focus_pair = index;
                            this.enter_viewer(cx);
                        }))
                        .child(
                            div()
                                .id(SharedString::from(format!("drag-{pair_id}")))
                                .w(px(22.))
                                .h(px(36.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_color(rgb(MUTED))
                                .cursor(CursorStyle::OpenHand)
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                        this.begin_row_drag(&drag_row, event.position, cx);
                                        cx.stop_propagation();
                                    }),
                                )
                                .on_click(|_: &ClickEvent, _, cx| cx.stop_propagation())
                                .child("⠿"),
                        )
                        .child(
                            div()
                                .w(px(28.))
                                .text_color(rgb(MUTED))
                                .child(SharedString::from((position + 1).to_string())),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .overflow_hidden()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(SharedString::from(name)),
                                )
                                .when(!label.is_empty(), |this| {
                                    this.child(
                                        div()
                                            .text_sm()
                                            .text_color(rgb(MUTED))
                                            .child(SharedString::from(label)),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("edit-row-{pair_id}")))
                                .px(px(8.))
                                .h(px(28.))
                                .rounded(px(6.))
                                .flex()
                                .items_center()
                                .text_sm()
                                .text_color(rgb(MUTED))
                                .cursor(CursorStyle::PointingHand)
                                .hover(|style| style.bg(rgb(HOVER)).text_color(rgb(TEXT)))
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.open_edit(pair_id, Screen::Library, cx);
                                    cx.stop_propagation();
                                }))
                                .child("Edit"),
                        )
                        .child(thumb(before_id, before, 96.0, 60.0))
                        .child(thumb(after_id, after, 96.0, 60.0)),
                        ),
                        )
                        .when(arrange, |this| {
                            this.child(trash_button(
                                SharedString::from(format!("trash-pair-{pair_id}")),
                                cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.remove_listed_pair(index, cx);
                                    cx.stop_propagation();
                                }),
                            ))
                        }),
                )
            })
            .when(arrange && !placeholder, |this| {
                this.child(arrange_box(
                    SharedString::from(format!("pick-pair-{pair_id}")),
                    picked,
                    28.0,
                    PAIR_PICK_LEFT,
                    cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.toggle_picked_pair(pair_id, cx);
                        cx.stop_propagation();
                    }),
                ))
            })
            .into_any_element()
    }

    fn pair_face(&self, row: &PairRow) -> gpui::AnyElement {
        div()
            .w_full()
            .overflow_hidden()
            .flex()
            .items_center()
            .gap(px(10.))
            .p(px(10.))
            .rounded(px(16.))
            .bg(rgb(0x2C2C2E))
            .border_1()
            .border_color(rgb(0x4A4A4E))
            .child(div().w(px(22.)).flex_shrink_0().text_color(rgb(MUTED)).child("⠿"))
            .child(
                div()
                    .w(px(28.))
                    .flex_shrink_0()
                    .text_color(rgb(MUTED))
                    .child(SharedString::from((row.position + 1).to_string())),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .min_w(px(0.))
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(SharedString::from(row.name.clone())),
                    )
                    .when(!row.label.is_empty(), |this| {
                        this.child(
                            div()
                                .min_w(px(0.))
                                .truncate()
                                .text_sm()
                                .text_color(rgb(MUTED))
                                .child(SharedString::from(row.label.clone())),
                        )
                    }),
            )
            .child(thumb(row.before_id, row.before.clone(), 96.0, 60.0))
            .child(thumb(row.after_id, row.after.clone(), 96.0, 60.0))
            .into_any_element()
    }

    fn drag_layer(&mut self, _cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(drag) = self.row_drag.as_ref() else {
            return div().into_any_element();
        };
        if !drag.moved {
            return div().into_any_element();
        }
        let left = px_f(drag.cursor.x) - drag.grab_x;
        let top = px_f(drag.cursor.y) - drag.grab_y;
        let width = drag.width;
        let pair_id = drag.pair_id;
        let Some(row) = self.listed_row_by_id(pair_id) else {
            return div().into_any_element();
        };
        div()
            .absolute()
            .left(px(left))
            .top(px(top))
            .w(px(width))
            .overflow_hidden()
            .child(self.pair_face(&row))
            .into_any_element()
    }

    fn comparison(
        &mut self,
        index: usize,
        frame_w: f32,
        frame_h: f32,
        interactive: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let pair = &self.library.pairs[index];
        let split = pair.split;
        let before_id = pair.before;
        let after_id = pair.after;
        let before = self.library.photo(before_id).unwrap().path.clone();
        let after = self.library.photo(after_id).unwrap().path.clone();
        let clip_w = frame_w * split;
        let fit_mode = self.fit;
        let wipe_before = before.clone();
        let wipe_after = after.clone();
        let wipe = self.compare_mode == CompareMode::Wipe;
        let blend = self.blend.clamp(0.0, 1.0);
        let frame = self.frame.clone();
        let entity = cx.weak_entity();
        let pins = if interactive && self.notes_open {
            pair.pins.clone()
        } else {
            Vec::new()
        };
        let placing = interactive && self.placing;
        let mut marks = Vec::new();
        if interactive && self.notes_open {
            for (number, pin) in pins.iter().enumerate() {
                marks.push(self.pin_mark(pin, number + 1, frame_w, frame_h, cx));
            }
        }

        div()
            .relative()
            .w(px(frame_w))
            .h(px(frame_h))
            .rounded(px(16.))
            .overflow_hidden()
            .bg(rgb(0x0E0E0E))
            .when(interactive, |this| {
                this.child(
                    canvas(
                        |bounds, _, _| bounds,
                        move |bounds, _, window, _| {
                            frame.set(Some(bounds));
                            let move_entity = entity.clone();
                            window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                                if phase != DispatchPhase::Bubble {
                                    return;
                                }
                                let _ = move_entity.update(cx, |this, cx| {
                                    this.track_move(event.position, cx);
                                });
                            });
                            let up_entity = entity.clone();
                            window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                                if phase != DispatchPhase::Bubble
                                    || event.button != MouseButton::Left
                                {
                                    return;
                                }
                                let _ = up_entity.update(cx, |this, cx| this.track_up(cx));
                            });
                        },
                    )
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
                )
            })
            .when(wipe, |this| {
                this.child(placed_image(
                    SharedString::from(format!("after-{after_id}-{index}")),
                    wipe_after,
                    0.0,
                    frame_w,
                    frame_h,
                    object_fit(fit_mode),
                ))
                .child(
                    div()
                        .relative()
                        .absolute()
                        .top_0()
                        .left_0()
                        .w(px(clip_w))
                        .h(px(frame_h))
                        .overflow_hidden()
                        .child(placed_image(
                            SharedString::from(format!("before-{before_id}-{index}")),
                            wipe_before,
                            0.0,
                            frame_w,
                            frame_h,
                            object_fit(fit_mode),
                        )),
                )
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left(px((clip_w - 0.5).clamp(0.0, (frame_w - 1.0).max(0.0))))
                        .w(px(1.))
                        .h(px(frame_h))
                        .bg(rgb(0xFFFFFF)),
                )
            })
            .when(!wipe, |this| {
                // After is stacked on Before. Opacity is After's share of the blend.
                this.child(placed_image(
                    SharedString::from(format!("before-{before_id}-{index}")),
                    before,
                    0.0,
                    frame_w,
                    frame_h,
                    object_fit(fit_mode),
                ))
                .child(
                    div()
                        .relative()
                        .absolute()
                        .top_0()
                        .left_0()
                        .w(px(frame_w))
                        .h(px(frame_h))
                        .opacity(blend)
                        .child(placed_image(
                            SharedString::from(format!("after-{after_id}-{index}")),
                            after,
                            0.0,
                            frame_w,
                            frame_h,
                            object_fit(fit_mode),
                        )),
                )
            })
            .when(interactive && wipe, |this| {
                this.child(side_badge("Before", true, frame_h))
                    .child(side_badge("After", false, frame_h))
                    .child(mask_handle(index, clip_w, frame_h, cx))
            })
            .children(marks)
            .when(placing, |this| this.child(self.pin_catch(cx)))
            .into_any_element()
    }
}

fn side_badge(label: &'static str, leading: bool, _frame_h: f32) -> gpui::AnyElement {
    div()
        .absolute()
        .top(px(14.))
        .when(leading, |this| this.left(px(14.)))
        .when(!leading, |this| this.right(px(14.)))
        .px(px(10.))
        .py(px(4.))
        .rounded(px(8.))
        .bg(rgb(0x111111).opacity(0.88))
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .child(label)
        .into_any_element()
}

fn mask_handle(index: usize, clip_w: f32, frame_h: f32, cx: &mut Context<Studio>) -> gpui::AnyElement {
    const DIAMETER: f32 = 36.0;
    const HIT: f32 = 44.0;
    div()
        .id(SharedString::from(format!("mask-{index}")))
        .absolute()
        .top_0()
        .left(px(clip_w - HIT / 2.0))
        .w(px(HIT))
        .h(px(frame_h))
        .cursor(CursorStyle::ResizeLeftRight)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, event: &MouseDownEvent, _, cx| {
                this.begin_drag(event.position, cx);
                cx.stop_propagation();
            }),
        )
        .child(
            div()
                .absolute()
                .top(px((frame_h - DIAMETER) / 2.0))
                .left(px((HIT - DIAMETER) / 2.0))
                .size(px(DIAMETER))
                .rounded_full()
                .bg(rgb(0xFFFFFF))
                .flex()
                .items_center()
                .justify_center()
                .gap(px(1.))
                .child(icon(CHEVRON_LEFT, 12., 0x1A1A1A))
                .child(icon(CHEVRON_RIGHT, 12., 0x1A1A1A)),
        )
        .into_any_element()
}

fn placed_image(
    id: SharedString,
    path: std::path::PathBuf,
    left: f32,
    width: f32,
    height: f32,
    fit: ObjectFit,
) -> gpui::AnyElement {
    img(path)
        .id(id)
        .absolute()
        .top_0()
        .left(px(left))
        .w(px(width))
        .h(px(height))
        .object_fit(fit)
        .into_any_element()
}

fn image_card(
    id: u64,
    path: std::path::PathBuf,
    label: String,
    width: f32,
    height: f32,
    selectable: bool,
    selected: bool,
    removable: bool,
    pick: Option<bool>,
    cx: &mut Context<Studio>,
) -> gpui::AnyElement {
    let caption = if selectable {
        pretty_caption(std::path::Path::new(&label))
    } else {
        label
    };
    let border = if selected { BLUE } else { LINE };
    let card_h = height + 44.0;
    let marked = pick;
    div()
        .id(SharedString::from(format!("card-{id}")))
        .relative()
        .w(px(width))
        .h(px(card_h))
        .flex()
        .flex_col()
        .gap(px(8.))
        .p(px(8.))
        .overflow_hidden()
        .rounded(px(16.))
        .bg(rgb(CARD))
        .border(px(if selected { 2. } else { 1. }))
        .border_color(rgb(border))
        .cursor(CursorStyle::PointingHand)
        .hover(|style| style.bg(rgb(0x222224)))
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            if marked.is_some() {
                this.toggle_picked_photo(id, cx);
            } else {
                this.choose_photo(id, !selectable);
            }
            cx.stop_propagation();
        }))
        .child(
            div()
                .relative()
                .w(px(width - 16.0))
                .h(px(height))
                .rounded(px(10.))
                .overflow_hidden()
                .bg(rgb(0x101010))
                .children(list_picture(id, &path, "card"))
                .when(selected, |this| {
                    this.child(
                        div()
                            .absolute()
                            .top(px(6.))
                            .right(px(6.))
                            .size(px(22.))
                            .rounded_full()
                            .bg(rgb(BLUE))
                            .border(px(2.))
                            .border_color(rgb(0xFFFFFF))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_xs()
                            .text_color(rgb(0xFFFFFF))
                            .font_weight(FontWeight::BOLD)
                            .child("✓"),
                    )
                }),
        )
        .child(
            div()
                .h(px(20.))
                .w_full()
                .px(px(4.))
                .flex()
                .items_center()
                .gap(px(6.))
                .overflow_hidden()
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_sm()
                        .truncate()
                        .child(SharedString::from(caption)),
                )
                .when(removable, |this| this.child(photo_trash(id, cx))),
        )
        .when(pick.is_some(), |this| {
            let on = pick.unwrap();
            this.child(arrange_box(
                SharedString::from(format!("pick-photo-{id}")),
                on,
                PHOTO_PICK_TOP,
                PHOTO_PICK_LEFT,
                cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.toggle_picked_photo(id, cx);
                    cx.stop_propagation();
                }),
            ))
        })
        .into_any_element()
}

/// True when every listed pair and every loose photo is already marked.
fn selection_covers(
    pairs: &[u64],
    photos: &[u64],
    picked_pairs: &[u64],
    picked_photos: &[u64],
) -> bool {
    if pairs.is_empty() && photos.is_empty() {
        return false;
    }
    pairs.iter().all(|id| picked_pairs.contains(id))
        && photos.iter().all(|id| picked_photos.contains(id))
}

/// Pairs marked on the library, in library order, for the new-group dialog.
fn seeded_group_picks(pair_ids: &[u64], picked: &[u64]) -> Vec<u64> {
    pair_ids
        .iter()
        .copied()
        .filter(|id| picked.contains(id))
        .collect()
}

/// Label for the new-group dialog. Deselect all only when every pair is checked.
fn group_select_label(pair_ids: &[u64], picks: &[u64]) -> &'static str {
    if group_selection_covers(pair_ids, picks) {
        "Deselect all"
    } else {
        "Select all"
    }
}

/// Next check set for the new-group dialog. Select all uses library order.
fn next_group_picks(pair_ids: &[u64], picks: &[u64]) -> Vec<u64> {
    if group_selection_covers(pair_ids, picks) {
        Vec::new()
    } else {
        pair_ids.to_vec()
    }
}

fn group_selection_covers(pair_ids: &[u64], picks: &[u64]) -> bool {
    !pair_ids.is_empty() && pair_ids.iter().all(|id| picks.contains(id))
}

fn select_all_label(
    pairs: &[u64],
    photos: &[u64],
    picked_pairs: &[u64],
    picked_photos: &[u64],
) -> &'static str {
    if selection_covers(pairs, photos, picked_pairs, picked_photos) {
        "Deselect all"
    } else {
        "Select all"
    }
}

fn toggle_id(ids: &mut Vec<u64>, id: u64) {
    if let Some(index) = ids.iter().position(|item| *item == id) {
        ids.remove(index);
    } else {
        ids.push(id);
    }
}

/// Edit-mode mark. It is painted over the item and does not take a layout slot.
fn arrange_box(
    id: SharedString,
    on: bool,
    top: f32,
    left: f32,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui::AnyElement {
    div()
        .id(id)
        .absolute()
        .top(px(top))
        .left(px(left))
        .size(px(PAIR_PICK_SIZE))
        .rounded(px(7.))
        .flex()
        .items_center()
        .justify_center()
        .occlude()
        .cursor(CursorStyle::PointingHand)
        .bg(rgb(0x000000).opacity(0.55))
        .border_1()
        .border_color(rgb(0xFFFFFF).opacity(0.92))
        .text_color(rgb(0xFFFFFF))
        .text_sm()
        .font_weight(FontWeight::BOLD)
        .when(!on, |this| {
            this.hover(|style| style.bg(rgb(0x000000).opacity(0.75)))
        })
        .when(on, |this| this.bg(rgb(BLUE).opacity(0.92)).child("✓"))
        .on_click(on_click)
        .into_any_element()
}

fn photo_trash(id: u64, cx: &mut Context<Studio>) -> gpui::AnyElement {
    div()
        .id(SharedString::from(format!("trash-photo-{id}")))
        .w(px(20.))
        .h(px(20.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(5.))
        .occlude()
        .cursor(CursorStyle::PointingHand)
        .hover(|style| style.bg(rgb(RED).opacity(0.18)))
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.remove_loose_photo(id, cx);
            cx.stop_propagation();
        }))
        .child(icon(TRASH_ICON, 14., RED))
        .into_any_element()
}

fn trash_button(
    id: SharedString,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui::AnyElement {
    div()
        .id(id)
        .w(px(32.))
        .h(px(32.))
        .flex_shrink_0()
        .rounded(px(8.))
        .flex()
        .items_center()
        .justify_center()
        .occlude()
        .cursor(CursorStyle::PointingHand)
        .hover(|style| style.bg(rgb(RED).opacity(0.18)))
        .on_click(on_click)
        .child(icon(TRASH_ICON, 16., RED))
        .into_any_element()
}

fn pair_check_row(
    pair_id: u64,
    name: String,
    before: std::path::PathBuf,
    after: std::path::PathBuf,
    before_id: u64,
    after_id: u64,
    checked: bool,
    cx: &mut Context<Studio>,
) -> gpui::AnyElement {
    div()
        .id(SharedString::from(format!("pick-{pair_id}")))
        .h(px(84.))
        .w_full()
        .flex()
        .items_center()
        .gap(px(12.))
        .p(px(10.))
        .rounded(px(14.))
        .bg(rgb(CARD))
        .border_1()
        .border_color(rgb(LINE))
        .cursor(CursorStyle::PointingHand)
        .hover(|style| style.bg(rgb(0x222224)))
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.toggle_group_pick(pair_id, cx);
        }))
        .child(checkbox(checked))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(SharedString::from(name)),
        )
        .child(thumb(before_id, before, 84.0, 56.0))
        .child(div().w(px(6.)))
        .child(thumb(after_id, after, 84.0, 56.0))
        .into_any_element()
}

fn list_picture(id: u64, asset: &Path, kind: &str) -> Option<gpui::AnyElement> {
    ready_thumb(asset).map(|path| {
        img(path)
            .id(SharedString::from(format!("{kind}-{id}")))
            .size_full()
            .object_fit(ObjectFit::Contain)
            .into_any_element()
    })
}

fn thumb(id: u64, path: PathBuf, width: f32, height: f32) -> gpui::AnyElement {
    div()
        .w(px(width))
        .h(px(height))
        .rounded(px(8.))
        .overflow_hidden()
        .bg(rgb(0x101010))
        .flex_shrink_0()
        .children(list_picture(id, &path, "mini"))
        .into_any_element()
}

fn checkbox(on: bool) -> gpui::AnyElement {
    div()
        .size(px(18.))
        .rounded(px(5.))
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .text_xs()
        .font_weight(FontWeight::BOLD)
        .when(on, |this| {
            this.bg(rgb(BLUE))
                .text_color(rgb(0xFFFFFF))
                .child("✓")
        })
        .when(!on, |this| this.border_1().border_color(rgb(0x6E6E73)))
        .into_any_element()
}

fn viewer_group_row(
    id: u64,
    name: String,
    count: usize,
    checked: bool,
    cx: &mut Context<Studio>,
) -> gpui::AnyElement {
    div()
        .id(SharedString::from(format!("visible-group-{id}")))
        .h(px(36.))
        .px(px(14.))
        .flex()
        .items_center()
        .gap(px(10.))
        .cursor(CursorStyle::PointingHand)
        .hover(|style| style.bg(rgb(0x2E2E32)))
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.toggle_viewer_group(id, cx);
            cx.stop_propagation();
        }))
        .child(checkbox(checked))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .child(SharedString::from(name)),
        )
        .child(
            div()
                .text_color(rgb(MUTED))
                .child(SharedString::from(count.to_string())),
        )
        .into_any_element()
}

fn sidebar_row(
    id: SharedString,
    icon: Option<&'static [u8]>,
    label: SharedString,
    count: usize,
    selected: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui::AnyElement {
    div()
        .id(id)
        .h(px(34.))
        .px(px(10.))
        .flex()
        .items_center()
        .gap(px(8.))
        .rounded(px(8.))
        .cursor(CursorStyle::PointingHand)
        .text_sm()
        .when(selected, |this| this.bg(rgb(0x2A2A2C)))
        .hover(|style| style.bg(rgb(if selected { 0x343438 } else { 0x242424 })))
        .on_click(on_click)
        .when(icon.is_some(), |this| {
            this.child(
                svg()
                    .data(icon.unwrap())
                    .w(px(15.))
                    .h(px(15.))
                    .text_color(rgb(TEXT)),
            )
        })
        .child(div().flex_1().child(label))
        .child(
            div()
                .text_color(rgb(MUTED))
                .child(SharedString::from(count.to_string())),
        )
        .into_any_element()
}

fn quiet_pill(
    id: &'static str,
    label: &'static str,
    mark: Option<&'static [u8]>,
    enabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui::AnyElement {
    let ink = if enabled { TEXT } else { MUTED };
    div()
        .id(id)
        .h(px(34.))
        .px(px(14.))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(8.))
        .text_sm()
        .font_weight(FontWeight::MEDIUM)
        .bg(rgb(0x2C2C2E))
        .when(enabled, |this| {
            this.text_color(rgb(TEXT))
                .cursor(CursorStyle::PointingHand)
                .hover(|style| style.bg(rgb(HOVER)))
                .on_click(on_click)
        })
        .when(!enabled, |this| this.text_color(rgb(MUTED)).opacity(0.65))
        .when(mark.is_some(), |this| this.child(icon(mark.unwrap(), 15., ink)))
        .child(label)
        .into_any_element()
}

fn pill(
    id: &'static str,
    label: &'static str,
    mark: Option<&'static [u8]>,
    enabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui::AnyElement {
    let ink = if enabled { 0x111111 } else { MUTED };
    div()
        .id(id)
        .h(px(36.))
        .px(px(16.))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(8.))
        .text_sm()
        .font_weight(FontWeight::SEMIBOLD)
        .when(enabled, |this| {
            this.bg(rgb(0xF5F5F7))
                .text_color(rgb(0x111111))
                .cursor(CursorStyle::PointingHand)
                .hover(|style| style.bg(rgb(0xFFFFFF)))
                .on_click(on_click)
        })
        .when(!enabled, |this| {
            this.bg(rgb(0x2A2A2C))
                .text_color(rgb(MUTED))
                .opacity(0.7)
        })
        .when(mark.is_some(), |this| this.child(icon(mark.unwrap(), 15., ink)))
        .child(label)
        .into_any_element()
}

fn edit_text_button(
    id: &'static str,
    label: SharedString,
    danger: bool,
    enabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui::AnyElement {
    let color = if !enabled {
        MUTED
    } else if danger {
        RED
    } else {
        TEXT
    };
    div()
        .id(id)
        .h(px(32.))
        .px(px(10.))
        .rounded(px(8.))
        .flex()
        .items_center()
        .text_sm()
        .font_weight(FontWeight::MEDIUM)
        .text_color(rgb(color))
        .when(!enabled, |this| this.opacity(0.55))
        .when(enabled, |this| {
            this.cursor(CursorStyle::PointingHand)
                .hover(|style| {
                    if danger {
                        style.bg(rgb(RED).opacity(0.18))
                    } else {
                        style.bg(rgb(HOVER))
                    }
                })
                .on_click(on_click)
        })
        .child(label)
        .into_any_element()
}

fn danger_pill(
    id: &'static str,
    label: &'static str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui::AnyElement {
    div()
        .id(id)
        .h(px(36.))
        .px(px(16.))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(rgb(RED))
        .text_color(rgb(0xFFFFFF))
        .text_sm()
        .font_weight(FontWeight::SEMIBOLD)
        .cursor(CursorStyle::PointingHand)
        .hover(|style| style.bg(rgb(0xFF6961)))
        .on_click(on_click)
        .child(label)
        .into_any_element()
}

fn text_button(
    id: &'static str,
    label: &'static str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui::AnyElement {
    div()
        .id(id)
        .h(px(32.))
        .px(px(10.))
        .rounded(px(8.))
        .flex()
        .items_center()
        .cursor(CursorStyle::PointingHand)
        .text_sm()
        .font_weight(FontWeight::MEDIUM)
        .hover(|style| style.bg(rgb(HOVER)))
        .on_click(on_click)
        .child(label)
        .into_any_element()
}

fn icon(data: &'static [u8], size: f32, color: u32) -> gpui::AnyElement {
    svg()
        .data(data)
        .w(px(size))
        .h(px(size))
        .text_color(rgb(color))
        .into_any_element()
}

fn icon_button(
    id: &'static str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui::AnyElement {
    div()
        .id(id)
        .w(px(32.))
        .h(px(32.))
        .rounded(px(8.))
        .flex()
        .items_center()
        .justify_center()
        .cursor(CursorStyle::PointingHand)
        .hover(|style| style.bg(rgb(HOVER)))
        .on_click(on_click)
        .child(icon(PLUS_ICON, 18., TEXT))
        .into_any_element()
}

fn fit_option(
    label: &'static str,
    active: bool,
    mode: FitMode,
    cx: &mut Context<Studio>,
) -> gpui::AnyElement {
    div()
        .id(SharedString::from(format!("fit-{label}")))
        .h(px(32.))
        .px(px(12.))
        .flex()
        .items_center()
        .cursor(CursorStyle::PointingHand)
        .text_color(rgb(if active { TEXT } else { MUTED }))
        .hover(|style| style.bg(rgb(0x2E2E32)))
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.fit = mode;
            this.fit_menu = false;
            this.persist();
            cx.notify();
            cx.stop_propagation();
        }))
        .child(label)
        .into_any_element()
}

fn plural<'a>(count: usize, one: &'a str, many: &'a str) -> &'a str {
    if count == 1 { one } else { many }
}

fn px_f(value: Pixels) -> f32 {
    f32::from(value)
}

struct EscapeState {
    placing: bool,
    row_drag: bool,
    search: bool,
    auto_pair: bool,
    menu: bool,
    focused: bool,
    notes: bool,
    arrange: bool,
    confirm: bool,
    screen: Screen,
}

enum EscapeAction {
    CancelPlace,
    CancelDrag,
    CloseSearch,
    CloseAutoPair,
    CloseMenu,
    Blur,
    CloseNotes,
    CloseConfirm,
    CloseArrange,
    LeaveViewer,
    CancelCreate,
    CancelEdit,
    CancelGroup,
    EnterViewer,
}

fn escape_action(state: &EscapeState) -> EscapeAction {
    if state.confirm {
        EscapeAction::CloseConfirm
    } else if state.placing {
        EscapeAction::CancelPlace
    } else if state.row_drag {
        EscapeAction::CancelDrag
    } else if state.search {
        EscapeAction::CloseSearch
    } else if state.auto_pair {
        EscapeAction::CloseAutoPair
    } else if state.menu {
        EscapeAction::CloseMenu
    } else if state.focused {
        EscapeAction::Blur
    } else if state.notes {
        EscapeAction::CloseNotes
    } else if state.arrange && state.screen == Screen::Library {
        EscapeAction::CloseArrange
    } else {
        match state.screen {
            Screen::Viewer => EscapeAction::LeaveViewer,
            Screen::Create => EscapeAction::CancelCreate,
            Screen::Edit => EscapeAction::CancelEdit,
            Screen::NewGroup => EscapeAction::CancelGroup,
            Screen::Library => EscapeAction::EnterViewer,
        }
    }
}

enum EnterAction {
    OpenSearch,
    CreateAutoPairs,
    SaveCreate,
    SaveEdit,
    CreateGroup,
    EnterViewer,
}

fn enter_action(search: bool, auto_pair: bool, screen: Screen) -> EnterAction {
    if search {
        EnterAction::OpenSearch
    } else if auto_pair {
        EnterAction::CreateAutoPairs
    } else {
        match screen {
            Screen::Edit => EnterAction::SaveEdit,
            Screen::Create => EnterAction::SaveCreate,
            Screen::NewGroup => EnterAction::CreateGroup,
            Screen::Library | Screen::Viewer => EnterAction::EnterViewer,
        }
    }
}

enum Typing {
    Pass,
    Stop,
    PasteText,
    Newline,
    Caret(i32),
    Backspace,
    Delete,
    Insert,
}

fn typing_effect(field: Option<Field>, key: &str, command: bool) -> Typing {
    let Some(field) = field else {
        return Typing::Pass;
    };
    let search = matches!(field, Field::Search);
    let note_body = matches!(field, Field::Note(_) | Field::PinBody(_));
    match key {
        "up" | "down" | "enter" | "escape" if search => Typing::Pass,
        "enter" if note_body => Typing::Newline,
        "enter" | "escape" => Typing::Pass,
        "up" | "down" => Typing::Stop,
        "left" => Typing::Caret(-1),
        "right" => Typing::Caret(1),
        "backspace" => Typing::Backspace,
        "delete" => Typing::Delete,
        "v" if command => Typing::PasteText,
        "f" if command => Typing::Pass,
        _ if command => Typing::Stop,
        _ => Typing::Insert,
    }
}

fn field_limit(field: Field) -> usize {
    match field {
        Field::Note(_) | Field::PinBody(_) => NOTE_LIMIT,
        _ => NAME_LIMIT,
    }
}

fn norm_rect(x0: f32, y0: f32, x1: f32, y1: f32) -> (f32, f32, f32, f32) {
    let left = x0.min(x1).clamp(0.0, 1.0);
    let top = y0.min(y1).clamp(0.0, 1.0);
    let right = x0.max(x1).clamp(0.0, 1.0);
    let bottom = y0.max(y1).clamp(0.0, 1.0);
    (left, top, (right - left).max(0.0), (bottom - top).max(0.0))
}

fn looks_like_image(bytes: &[u8], ext: &str) -> bool {
    let ext = ext.trim().trim_start_matches('.').to_ascii_lowercase();
    if bytes.len() < 4 {
        return false;
    }
    match ext.as_str() {
        "png" => bytes.starts_with(&[0x89, b'P', b'N', b'G']),
        "jpg" | "jpeg" => bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF,
        "gif" => bytes.starts_with(b"GIF8"),
        "webp" => bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
        "bmp" => bytes.starts_with(b"BM"),
        "tif" | "tiff" => bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*"),
        _ => false,
    }
}

fn shortcut(key: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("⌘{key}")
    } else {
        format!("Ctrl+{key}")
    }
}

fn create_label(count: usize) -> String {
    match count {
        0 => "Create pairs".to_string(),
        1 => "Create 1 pair".to_string(),
        count => format!("Create {count} pairs"),
    }
}

fn short_name(name: &str) -> String {
    let words: Vec<&str> = name.split_whitespace().collect();
    if words.len() > 3 {
        format!("{} {}", words[0], words[1])
    } else {
        name.to_string()
    }
}

fn side_name(side: SlotSide) -> &'static str {
    match side {
        SlotSide::Before => "Before",
        SlotSide::After => "After",
    }
}

fn scope_name(scope: SearchScope) -> &'static str {
    match scope {
        SearchScope::Current => "Current selection",
        SearchScope::All => "All pairs",
    }
}

fn caret_bar() -> gpui::AnyElement {
    div()
        .w(px(1.5))
        .h(px(16.))
        .flex_shrink_0()
        .bg(rgb(BLUE))
        .with_animation(
            "text-caret",
            Animation::new(Duration::from_millis(1000))
                .repeat()
                .with_max_fps(10.0),
            |bar, delta| bar.opacity(if delta < 0.5 { 1.0 } else { 0.0 }),
        )
        .into_any_element()
}

fn caret_runs(text: &str, caret: usize, focused: bool) -> gpui::AnyElement {
    let chars: Vec<char> = text.chars().collect();
    let caret = caret.min(chars.len());
    let before: String = chars[..caret].iter().collect();
    let after: String = chars[caret..].iter().collect();
    div()
        .flex()
        .items_center()
        .w_full()
        .min_h(px(18.))
        .child(SharedString::from(before))
        .when(focused, |this| this.child(caret_bar()))
        .child(SharedString::from(after))
        .into_any_element()
}

fn caret_block(text: &str, caret: usize, focused: bool) -> gpui::AnyElement {
    if !text.contains('\n') {
        return caret_runs(text, caret, focused);
    }
    let chars: Vec<char> = text.chars().collect();
    let caret = caret.min(chars.len());
    let before: String = chars[..caret].iter().collect();
    let after: String = chars[caret..].iter().collect();
    let before_lines: Vec<&str> = before.split('\n').collect();
    let after_lines: Vec<&str> = after.split('\n').collect();
    let last = before_lines.len().saturating_sub(1);
    let mut lines = Vec::new();
    for (index, line) in before_lines.iter().enumerate() {
        if index == last {
            let rest = after_lines.first().copied().unwrap_or("");
            lines.push(caret_runs(&format!("{line}{rest}"), line.chars().count(), focused));
        } else {
            lines.push(caret_runs(line, line.chars().count(), false));
        }
    }
    for line in after_lines.iter().skip(1) {
        lines.push(caret_runs(line, 0, false));
    }
    div()
        .flex()
        .flex_col()
        .w_full()
        .children(lines)
        .into_any_element()
}

fn order_option(order: PairOrder, current: PairOrder, cx: &mut Context<Studio>) -> gpui::AnyElement {
    let active = order == current;
    let id = match order {
        PairOrder::Custom => "order-custom",
        PairOrder::Name => "order-name",
        PairOrder::NameDesc => "order-name-desc",
        PairOrder::Oldest => "order-oldest",
        PairOrder::Newest => "order-newest",
    };
    div()
        .id(id)
        .h(px(32.))
        .px(px(12.))
        .flex()
        .items_center()
        .cursor(CursorStyle::PointingHand)
        .text_color(rgb(if active { TEXT } else { MUTED }))
        .hover(|style| style.bg(rgb(0x2E2E32)))
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.apply_pair_order(order, cx);
            cx.stop_propagation();
        }))
        .child(order.label())
        .into_any_element()
}

fn toast_bar(message: String, undo: bool, cx: &mut Context<Studio>) -> gpui::AnyElement {
    div()
        .absolute()
        .bottom(px(FOOTER + 16.0))
        .left_0()
        .right_0()
        .flex()
        .justify_center()
        .child(
            div()
                .px(px(14.))
                .h(px(36.))
                .rounded_full()
                .bg(rgb(0x2A2A2C))
                .border_1()
                .border_color(rgb(LINE))
                .flex()
                .items_center()
                .gap(px(12.))
                .text_sm()
                .when(undo, |this| {
                    this.child(div().text_color(rgb(0x30D158)).child("✓"))
                })
                .child(SharedString::from(message))
                .when(undo, |this| {
                    this.child(
                        div()
                            .id("undo-paste")
                            .text_color(rgb(BLUE))
                            .cursor(CursorStyle::PointingHand)
                            .hover(|style| style.opacity(0.72))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.undo_paste(cx)))
                            .child("Undo"),
                    )
                }),
        )
        .into_any_element()
}

fn mode_option(
    label: &'static str,
    active: bool,
    mode: CompareMode,
    cx: &mut Context<Studio>,
) -> gpui::AnyElement {
    div()
        .id(SharedString::from(format!("mode-{label}")))
        .h(px(32.))
        .px(px(12.))
        .flex()
        .items_center()
        .cursor(CursorStyle::PointingHand)
        .text_color(rgb(if active { TEXT } else { MUTED }))
        .hover(|style| style.bg(rgb(0x2E2E32)))
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.set_compare_mode(mode, cx);
            cx.stop_propagation();
        }))
        .child(label)
        .into_any_element()
}

fn scope_option(
    label: &'static str,
    scope: SearchScope,
    active: bool,
    cx: &mut Context<Studio>,
) -> gpui::AnyElement {
    div()
        .id(SharedString::from(format!("scope-{label}")))
        .h(px(32.))
        .px(px(12.))
        .flex()
        .items_center()
        .cursor(CursorStyle::PointingHand)
        .text_color(rgb(if active { TEXT } else { MUTED }))
        .hover(|style| style.bg(rgb(0x2E2E32)))
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.search_scope = scope;
            this.search_selected = 0;
            this.scope_menu = false;
            cx.stop_propagation();
            cx.notify();
        }))
        .child(label)
        .into_any_element()
}

fn number_badge(number: usize, selected: bool) -> gpui::AnyElement {
    div()
        .size(px(22.))
        .rounded_full()
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .text_xs()
        .font_weight(FontWeight::BOLD)
        .bg(rgb(if selected { BLUE } else { 0x3A3A3C }))
        .text_color(rgb(0xFFFFFF))
        .child(SharedString::from(number.to_string()))
        .into_any_element()
}

fn row_probe(store: Rc<RefCell<Vec<Bounds<Pixels>>>>, position: usize) -> gpui::AnyElement {
    canvas(
        |bounds, _, _| bounds,
        move |bounds, _, _, _| {
            let mut slots = store.borrow_mut();
            if slots.len() <= position {
                slots.resize(position + 1, bounds);
            } else {
                slots[position] = bounds;
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
    .into_any_element()
}

fn insert_line(top: bool) -> gpui::AnyElement {
    div()
        .absolute()
        .left(px(8.))
        .right(px(8.))
        .h(px(2.))
        .rounded_full()
        .bg(rgb(BLUE))
        .when(top, |this| this.top(px(-6.)))
        .when(!top, |this| this.bottom(px(-6.)))
        .into_any_element()
}

fn marked_text(text: &str, query: &str) -> gpui::AnyElement {
    div()
        .flex()
        .flex_wrap()
        .children(highlight_spans(text, query).into_iter().map(|(on, part)| {
            div()
                .when(on, |this| this.text_color(rgb(BLUE)))
                .child(SharedString::from(part))
                .into_any_element()
        }))
        .into_any_element()
}

fn auto_cell_file(file: Option<(u64, PathBuf, String)>) -> gpui::AnyElement {
    match file {
        Some((id, path, name)) => div()
            .flex()
            .items_center()
            .gap(px(8.))
            .w_full()
            .min_w(px(0.))
            .overflow_hidden()
            .child(thumb(id, path, 56.0, 40.0))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .text_sm()
                    .truncate()
                    .child(SharedString::from(name)),
            )
            .into_any_element(),
        None => auto_cell_dash(),
    }
}

fn auto_cell_dash() -> gpui::AnyElement {
    div()
        .text_sm()
        .text_color(rgb(MUTED))
        .child("—")
        .into_any_element()
}

fn auto_cell_note(text: &'static str) -> gpui::AnyElement {
    div()
        .w_full()
        .min_w(px(0.))
        .overflow_hidden()
        .text_sm()
        .text_color(rgb(MUTED))
        .truncate()
        .child(text)
        .into_any_element()
}

fn auto_cell_empty() -> gpui::AnyElement {
    div().into_any_element()
}

fn fake_select(label: &'static str, value: &'static str) -> gpui::AnyElement {
    div()
        .w(px(220.))
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(div().text_sm().text_color(rgb(MUTED)).child(label))
        .child(
            div()
                .h(px(36.))
                .px(px(12.))
                .rounded(px(10.))
                .border_1()
                .border_color(rgb(LINE))
                .bg(rgb(FIELD))
                .flex()
                .items_center()
                .justify_between()
                .child(value)
                .child(div().text_color(rgb(MUTED)).child("▾")),
        )
        .into_any_element()
}

fn labeled_pill(
    id: &'static str,
    label: String,
    enabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui::AnyElement {
    div()
        .id(id)
        .h(px(36.))
        .px(px(16.))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .text_sm()
        .font_weight(FontWeight::SEMIBOLD)
        .when(enabled, |this| {
            this.bg(rgb(0xF5F5F7))
                .text_color(rgb(0x111111))
                .cursor(CursorStyle::PointingHand)
                .hover(|style| style.bg(rgb(0xFFFFFF)))
                .on_click(on_click)
        })
        .when(!enabled, |this| {
            this.bg(rgb(0x2A2A2C)).text_color(rgb(MUTED)).opacity(0.7)
        })
        .child(SharedString::from(label))
        .into_any_element()
}

/// How many cards of `card_w` fit in `viewport_w` after `chrome` is reserved.
/// `chrome` is padding, plus the sidebar when that column is open.
fn grid_columns(viewport_w: f32, card_w: f32, gap: f32, chrome: f32) -> usize {
    let inner = (viewport_w - chrome).max(card_w);
    let stride = card_w + gap;
    if stride <= 0.0 {
        return 1;
    }
    ((inner + gap) / stride).floor().max(1.0) as usize
}

/// Pair-row insertion index. The cursor selects the row whose center is below it,
/// and a cursor past the last center stays on the last row.
fn row_target(content_y: f32, item_h: f32, count: usize) -> usize {
    if count == 0 || item_h <= 0.0 {
        return 0;
    }
    let raw = (content_y / item_h + 0.5).floor().max(0.0) as usize;
    raw.min(count - 1)
}

/// Heights of the previous peek, the current frame, and the next peek.
/// A missing neighbor is zero, so the current frame sits against that edge.
fn stack_heights(content_h: f32, has_prev: bool, has_next: bool) -> (f32, f32, f32) {
    let content_h = content_h.max(1.0);
    let neighbors = usize::from(has_prev) + usize::from(has_next);
    if neighbors == 0 {
        return (0.0, content_h, 0.0);
    }
    let gaps = GAP * neighbors as f32;
    let available = (content_h - gaps).max(1.0);
    // Neighbor strips keep a fifth of the previous peek so the current pair gets that space.
    let peek = ((content_h * 0.16).clamp(84.0, 120.0).min(available * 0.22)) * 0.2;
    let main = (available - peek * neighbors as f32).max(1.0);
    let peek = (available - main) / neighbors as f32;
    (
        if has_prev { peek } else { 0.0 },
        main,
        if has_next { peek } else { 0.0 },
    )
}

pub fn run() {
    application().run(|cx: &mut App| {
        cx.set_window_appearance(Some(WindowAppearance::Dark));
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-o", AddPhotos, None),
            KeyBinding::new("up", PrevGroup, None),
            KeyBinding::new("down", NextGroup, None),
            KeyBinding::new("left", NudgeLeft, None),
            KeyBinding::new("right", NudgeRight, None),
            KeyBinding::new("shift-left", SnapLeft, None),
            KeyBinding::new("shift-right", SnapRight, None),
            KeyBinding::new("enter", OpenViewer, None),
            KeyBinding::new("escape", ToggleMode, None),
            KeyBinding::new("backspace", Unlink, None),
            KeyBinding::new("delete", Unlink, None),
            KeyBinding::new("cmd-f", Find, Some("Compare")),
            KeyBinding::new("cmd-v", PasteImage, Some("Compare")),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.set_menus(vec![Menu {
            name: "Compare".into(),
            items: vec![
                MenuItem::action("Add Images…", AddPhotos),
                MenuItem::action("Quit Compare", Quit),
            ],
            disabled: false,
        }]);

        let bounds = Bounds::centered(None, size(px(1440.), px(900.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(960.), px(640.))),
                titlebar: Some(TitlebarOptions {
                    title: None,
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(16.), px(18.))),
                }),
                ..Default::default()
            },
            |window, cx| {
                let studio = cx.new(|cx| {
                    let focus = cx.focus_handle();
                    focus.focus(window, cx);
                    let mut studio = Studio::new(focus);
                    studio.warm_thumbs(cx);
                    studio
                });
                let saving = studio.downgrade();
                window.on_window_should_close(cx, move |_, cx| {
                    let _ = saving.update(cx, |studio, _| studio.persist());
                    true
                });
                studio
            },
        )
        .expect("open window");
        cx.activate(true);
    });
}

#[cfg(test)]
mod layout_tests {
    use super::{
        EscapeAction, EscapeState, EnterAction, Field, Screen, Typing, create_label, escape_action,
        enter_action, field_limit, grid_columns, looks_like_image, norm_rect, row_target, short_name,
        stack_heights, typing_effect,
    };

    #[test]
    fn new_group_starts_from_the_marked_pairs() {
        assert_eq!(super::seeded_group_picks(&[1, 2, 3], &[3, 1, 9]), vec![1, 3]);
        assert!(super::seeded_group_picks(&[1, 2], &[]).is_empty());
    }

    #[test]
    fn group_select_all_flips_when_every_pair_is_checked() {
        assert_eq!(super::group_select_label(&[1, 2], &[]), "Select all");
        assert_eq!(super::group_select_label(&[1, 2], &[1]), "Select all");
        assert_eq!(super::group_select_label(&[1, 2], &[2, 1, 9]), "Deselect all");
        assert_eq!(super::group_select_label(&[], &[]), "Select all");
        assert_eq!(super::next_group_picks(&[4, 1, 2], &[1]), vec![4, 1, 2]);
        assert!(super::next_group_picks(&[1, 2], &[2, 1]).is_empty());
    }

    #[test]
    fn select_all_flips_only_when_every_visible_item_is_marked() {
        let pairs = [1, 2];
        let photos = [8];
        assert_eq!(
            super::select_all_label(&pairs, &photos, &[], &[]),
            "Select all"
        );
        assert_eq!(
            super::select_all_label(&pairs, &photos, &[1, 2], &[]),
            "Select all"
        );
        assert_eq!(
            super::select_all_label(&pairs, &photos, &[1, 2], &[8]),
            "Deselect all"
        );
        assert_eq!(super::select_all_label(&[], &[], &[], &[]), "Select all");
    }

    #[test]
    fn arrange_pick_toggles_membership() {
        let mut ids = Vec::new();
        super::toggle_id(&mut ids, 4);
        assert_eq!(ids, vec![4]);
        super::toggle_id(&mut ids, 9);
        assert_eq!(ids, vec![4, 9]);
        super::toggle_id(&mut ids, 4);
        assert_eq!(ids, vec![9]);
    }

    #[test]
    fn first_group_has_nothing_above_and_the_middle_has_both() {
        let (prev, main, next) = stack_heights(800.0, false, true);
        assert_eq!(prev, 0.0);
        assert!((next - 24.0).abs() < 0.05);
        assert!(main > next);

        let (prev, main, next) = stack_heights(800.0, true, true);
        assert!((prev - 24.0).abs() < 0.05 && (next - 24.0).abs() < 0.05);
        assert!(main > prev);
        let gaps = 2.0 * super::GAP;
        assert!((prev + main + next + gaps - 800.0).abs() < 0.05);

        let (prev, main, next) = stack_heights(800.0, true, false);
        assert_eq!(next, 0.0);
        assert!(prev > 0.0);
        assert!(main > prev);

        let (prev, main, next) = stack_heights(800.0, false, false);
        assert_eq!((prev, next), (0.0, 0.0));
        assert_eq!(main, 800.0);
    }

    fn busy() -> EscapeState {
        EscapeState {
            placing: false,
            row_drag: false,
            search: false,
            auto_pair: false,
            menu: false,
            focused: false,
            notes: false,
            arrange: false,
            confirm: false,
            screen: Screen::Viewer,
        }
    }

    #[test]
    fn escape_closes_the_topmost_layer() {
        let mut state = busy();
        state.confirm = true;
        state.arrange = true;
        state.screen = Screen::Library;
        assert!(matches!(escape_action(&state), EscapeAction::CloseConfirm));
        state.confirm = false;
        state.arrange = false;
        state.screen = Screen::Viewer;
        state.placing = true;
        state.search = true;
        state.focused = true;
        assert!(matches!(escape_action(&state), EscapeAction::CancelPlace));
        state.placing = false;
        state.row_drag = true;
        assert!(matches!(escape_action(&state), EscapeAction::CancelDrag));
        state.row_drag = false;
        assert!(matches!(escape_action(&state), EscapeAction::CloseSearch));
        state.search = false;
        state.auto_pair = true;
        state.focused = true;
        assert!(matches!(escape_action(&state), EscapeAction::CloseAutoPair));
        state.auto_pair = false;
        state.menu = true;
        assert!(matches!(escape_action(&state), EscapeAction::CloseMenu));
        state.menu = false;
        assert!(matches!(escape_action(&state), EscapeAction::Blur));
        state.focused = false;
        state.notes = true;
        assert!(matches!(escape_action(&state), EscapeAction::CloseNotes));
        state.notes = false;
        assert!(matches!(escape_action(&state), EscapeAction::LeaveViewer));
        state.screen = Screen::Create;
        assert!(matches!(escape_action(&state), EscapeAction::CancelCreate));
        state.screen = Screen::Edit;
        assert!(matches!(escape_action(&state), EscapeAction::CancelEdit));
        state.screen = Screen::NewGroup;
        assert!(matches!(escape_action(&state), EscapeAction::CancelGroup));
        state.screen = Screen::Library;
        state.arrange = true;
        assert!(matches!(escape_action(&state), EscapeAction::CloseArrange));
        state.arrange = false;
        assert!(matches!(escape_action(&state), EscapeAction::EnterViewer));
    }

    #[test]
    fn enter_follows_search_then_the_screen() {
        assert!(matches!(
            enter_action(true, true, Screen::Library),
            EnterAction::OpenSearch
        ));
        assert!(matches!(
            enter_action(false, true, Screen::Viewer),
            EnterAction::CreateAutoPairs
        ));
        assert!(matches!(
            enter_action(false, false, Screen::Create),
            EnterAction::SaveCreate
        ));
        assert!(matches!(
            enter_action(false, false, Screen::Edit),
            EnterAction::SaveEdit
        ));
        assert!(matches!(
            enter_action(false, false, Screen::NewGroup),
            EnterAction::CreateGroup
        ));
        assert!(matches!(
            enter_action(false, false, Screen::Library),
            EnterAction::EnterViewer
        ));
    }

    #[test]
    fn typing_keeps_search_keys_and_edits_text() {
        assert!(matches!(typing_effect(None, "a", false), Typing::Pass));
        assert!(matches!(
            typing_effect(Some(Field::Search), "down", false),
            Typing::Pass
        ));
        assert!(matches!(
            typing_effect(Some(Field::Search), "enter", false),
            Typing::Pass
        ));
        assert!(matches!(
            typing_effect(Some(Field::Search), "escape", false),
            Typing::Pass
        ));
        assert!(matches!(
            typing_effect(Some(Field::Note(1)), "enter", false),
            Typing::Newline
        ));
        assert!(matches!(
            typing_effect(Some(Field::PinBody(2)), "enter", false),
            Typing::Newline
        ));
        assert!(matches!(
            typing_effect(Some(Field::PairName), "enter", false),
            Typing::Pass
        ));
        assert!(matches!(
            typing_effect(Some(Field::PairName), "up", false),
            Typing::Stop
        ));
        assert!(matches!(
            typing_effect(Some(Field::PairName), "left", false),
            Typing::Caret(-1)
        ));
        assert!(matches!(
            typing_effect(Some(Field::BeforePattern), "right", false),
            Typing::Caret(1)
        ));
        assert!(matches!(
            typing_effect(Some(Field::PairName), "backspace", false),
            Typing::Backspace
        ));
        assert!(matches!(
            typing_effect(Some(Field::GroupName), "delete", false),
            Typing::Delete
        ));
        assert!(matches!(
            typing_effect(Some(Field::PairName), "v", true),
            Typing::PasteText
        ));
        assert!(matches!(
            typing_effect(Some(Field::Search), "f", true),
            Typing::Pass
        ));
        assert!(matches!(
            typing_effect(Some(Field::PairName), "a", true),
            Typing::Stop
        ));
        assert!(matches!(
            typing_effect(Some(Field::PairName), "a", false),
            Typing::Insert
        ));
        assert_eq!(field_limit(Field::PairName), 80);
        assert_eq!(field_limit(Field::Note(1)), 500);
    }

    #[test]
    fn norm_rect_orders_and_clamps() {
        let (x, y, w, h) = norm_rect(0.8, 0.2, 0.2, 0.6);
        assert!((x - 0.2).abs() < 1e-5 && (y - 0.2).abs() < 1e-5);
        assert!((w - 0.6).abs() < 1e-5 && (h - 0.4).abs() < 1e-5);
        assert_eq!(norm_rect(-0.4, 0.2, 1.4, 0.2), (0.0, 0.2, 1.0, 0.0));
    }

    #[test]
    fn image_sniff_accepts_photo_bytes_only() {
        assert!(looks_like_image(&[0x89, b'P', b'N', b'G', 0, 0], "png"));
        assert!(looks_like_image(&[0xFF, 0xD8, 0xFF, 0xE0], "jpg"));
        assert!(looks_like_image(b"GIF89a....", "gif"));
        assert!(!looks_like_image(b"<svg", "svg"));
        assert!(!looks_like_image(&[0x89, b'P', b'N', b'G'], "jpg"));
        assert!(!looks_like_image(&[1, 2, 3], "png"));
    }

    #[test]
    fn labels_follow_the_count_and_the_name() {
        assert_eq!(create_label(0), "Create pairs");
        assert_eq!(create_label(1), "Create 1 pair");
        assert_eq!(create_label(5), "Create 5 pairs");
        assert_eq!(short_name("House"), "House");
        assert_eq!(short_name("One Two Three"), "One Two Three");
        assert_eq!(
            short_name("Minimal Desk With Laptop And Mug"),
            "Minimal Desk"
        );
    }

    #[test]
    fn five_columns_fit_the_library_at_1440() {
        assert_eq!(grid_columns(1440.0, 236.0, 16.0, 56.0), 5);
        assert_eq!(grid_columns(1440.0, 236.0, 16.0, 276.0), 4);
        assert_eq!(grid_columns(200.0, 236.0, 16.0, 56.0), 1);
    }

    #[test]
    fn row_target_uses_the_row_center_and_clamps() {
        assert_eq!(row_target(0.0, 104.0, 5), 0);
        assert_eq!(row_target(51.0, 104.0, 5), 0);
        assert_eq!(row_target(53.0, 104.0, 5), 1);
        assert_eq!(row_target(1000.0, 104.0, 5), 4);
        assert_eq!(row_target(-20.0, 104.0, 5), 0);
        assert_eq!(row_target(10.0, 104.0, 0), 0);
    }
}
