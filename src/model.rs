//! Photos and the before/after pairs that link them.
//!
//! Before is always the left side of a pair. After is always the right side.
//! A photo belongs to at most one pair.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

pub const SPLIT_MIN: f32 = 0.0;
pub const SPLIT_MAX: f32 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PhotoSource {
    #[default]
    File,
    Clipboard,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Photo {
    pub id: u64,
    pub path: PathBuf,
    #[serde(default)]
    pub source: PhotoSource,
    /// Filename the user imported. Empty means the path's own file name.
    #[serde(default)]
    pub label: String,
}

/// A general note about a pair. It has no mark on the image.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairNote {
    pub id: u64,
    pub text: String,
}

/// A numbered mark stored in comparison-frame coordinates, each axis 0 to 1.
/// A point has zero width and height. A rectangle has a positive size.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pin {
    pub id: u64,
    pub title: String,
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CompareMode {
    #[default]
    Wipe,
    Overlay,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pair {
    pub id: u64,
    pub before: u64,
    pub after: u64,
    /// Horizontal mask. 0 is all after, 1 is all before. Starts centered.
    pub split: f32,
    pub name: String,
    #[serde(default)]
    pub notes: Vec<PairNote>,
    #[serde(default)]
    pub pins: Vec<Pin>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    pub id: u64,
    pub name: String,
    pairs: Vec<u64>,
}

impl Group {
    #[cfg(test)]
    pub fn pair_ids(&self) -> &[u64] {
        &self.pairs
    }

    pub fn pair_count(&self) -> usize {
        self.pairs.len()
    }
}

#[derive(Debug, Default)]
pub struct Library {
    photos: Vec<Photo>,
    pub pairs: Vec<Pair>,
    groups: Vec<Group>,
    pub draft_before: Option<u64>,
    pub draft_after: Option<u64>,
    next_id: u64,
    custom_order: bool,
    order: PairOrder,
}

/// How the pair list is arranged. Custom is the order left by dragging.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PairOrder {
    #[default]
    Custom,
    Name,
    NameDesc,
    Oldest,
    Newest,
}

impl PairOrder {
    pub fn label(self) -> &'static str {
        match self {
            Self::Custom => "Custom",
            Self::Name => "Name",
            Self::NameDesc => "Name (Z–A)",
            Self::Oldest => "Oldest",
            Self::Newest => "Newest",
        }
    }
}

fn pair_rank(left: &Pair, right: &Pair, order: PairOrder) -> std::cmp::Ordering {
    let folded = |pair: &Pair| pair.name.to_lowercase();
    match order {
        PairOrder::Name => folded(left).cmp(&folded(right)).then(left.id.cmp(&right.id)),
        PairOrder::NameDesc => folded(right).cmp(&folded(left)).then(right.id.cmp(&left.id)),
        PairOrder::Newest => right.id.cmp(&left.id),
        PairOrder::Oldest | PairOrder::Custom => left.id.cmp(&right.id),
    }
}

impl Library {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn photos(&self) -> &[Photo] {
        &self.photos
    }

    pub fn photo(&self, id: u64) -> Option<&Photo> {
        self.photos.iter().find(|photo| photo.id == id)
    }

    pub fn groups(&self) -> &[Group] {
        &self.groups
    }

    pub fn group(&self, id: u64) -> Option<&Group> {
        self.groups.iter().find(|group| group.id == id)
    }

    pub fn pair_by_id(&self, id: u64) -> Option<&Pair> {
        self.pairs.iter().find(|pair| pair.id == id)
    }

    /// Group names for a pair, in the order the groups were created.
    pub fn group_label(&self, pair_id: u64) -> String {
        self.groups
            .iter()
            .filter(|group| group.pairs.contains(&pair_id))
            .map(|group| group.name.as_str())
            .collect::<Vec<_>>()
            .join(" · ")
    }

    pub fn pair_index(&self, id: u64) -> Option<usize> {
        self.pairs
            .iter()
            .position(|pair| pair.before == id || pair.after == id)
    }

    /// Photos still waiting to be linked, in the order they were added.
    pub fn unassigned(&self) -> Vec<&Photo> {
        self.photos
            .iter()
            .filter(|photo| self.is_loose(photo.id))
            .collect()
    }

    fn is_loose(&self, id: u64) -> bool {
        self.pair_index(id).is_none()
    }

    /// Remember a photo that is already something GPUI can decode.
    /// Tests use this to avoid copying files. Imports go through `insert_archived`.
    #[cfg(test)]
    pub fn insert(&mut self, path: PathBuf) -> Option<u64> {
        if !is_image(&path) {
            return None;
        }
        if self.photos.iter().any(|photo| photo.path == path) {
            return None;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.photos.push(Photo {
            id,
            path,
            source: PhotoSource::File,
            label: String::new(),
        });
        Some(id)
    }

    /// Remember a local copy of an imported file, under its original filename.
    pub fn insert_archived(&mut self, path: PathBuf, label: String) -> Option<u64> {
        if !is_image(&path) {
            return None;
        }
        if self.photos.iter().any(|photo| photo.path == path) {
            return None;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.photos.push(Photo {
            id,
            path,
            source: PhotoSource::File,
            label,
        });
        Some(id)
    }

    /// Remember an image that already lives in local asset storage.
    pub fn insert_clipboard(&mut self, path: PathBuf) -> Option<u64> {
        if !is_image(&path) {
            return None;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.photos.push(Photo {
            id,
            path,
            source: PhotoSource::Clipboard,
            label: String::new(),
        });
        Some(id)
    }

    pub fn add_prepared(&mut self, files: impl IntoIterator<Item = (PathBuf, String)>) -> usize {
        let mut added = 0;
        for (path, label) in files {
            if self.insert_archived(path, label).is_some() {
                added += 1;
            }
        }
        added
    }

    /// Pick a loose photo for the pair being built.
    /// The first pick is before. The second is after. Picking either again clears that side.
    /// A third pick replaces after.
    pub fn select_for_draft(&mut self, id: u64) {
        if self.photo(id).is_none() || self.pair_index(id).is_some() {
            return;
        }
        if self.draft_before == Some(id) {
            self.draft_before = None;
            return;
        }
        if self.draft_after == Some(id) {
            self.draft_after = None;
            return;
        }
        if self.draft_before.is_none() {
            self.draft_before = Some(id);
        } else if self.draft_after.is_none() {
            self.draft_after = Some(id);
        } else {
            self.draft_after = Some(id);
        }
    }

    pub fn clear_draft(&mut self) {
        self.draft_before = None;
        self.draft_after = None;
    }

    /// Commit the two drafted photos. An empty name falls back to the before photo's title.
    pub fn commit_draft(&mut self, name: &str) -> Option<usize> {
        let before = self.draft_before?;
        let after = self.draft_after?;
        if before == after || self.photo(before).is_none() || self.photo(after).is_none() {
            return None;
        }
        let name = {
            let trimmed = name.trim();
            if trimmed.is_empty() {
                suggest_name(Path::new(&photo_label(self.photo(before).unwrap())))
            } else {
                trimmed.to_string()
            }
        };
        self.draft_before = None;
        self.draft_after = None;
        let id = self.next_id;
        self.next_id += 1;
        self.pairs.push(Pair {
            id,
            before,
            after,
            split: 0.5,
            name,
            notes: Vec::new(),
            pins: Vec::new(),
        });
        self.keep_named_order();
        self.pairs.iter().position(|pair| pair.id == id)
    }

    /// Replace the images or name of an existing pair without moving it or dropping its notes.
    pub fn update_pair(&mut self, id: u64, name: &str, before: u64, after: u64) -> bool {
        if before == after || self.photo(before).is_none() || self.photo(after).is_none() {
            return false;
        }
        if self.pair_index(before).is_some_and(|index| self.pairs[index].id != id) {
            return false;
        }
        if self.pair_index(after).is_some_and(|index| self.pairs[index].id != id) {
            return false;
        }
        let Some(pair) = self.pairs.iter_mut().find(|pair| pair.id == id) else {
            return false;
        };
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            pair.name = trimmed.to_string();
        }
        pair.before = before;
        pair.after = after;
        true
    }

    pub fn note_count(&self, pair_id: u64) -> usize {
        self.pair_by_id(pair_id)
            .map(|pair| pair.notes.len() + pair.pins.len())
            .unwrap_or(0)
    }

    pub fn add_note(&mut self, pair_id: u64) -> Option<u64> {
        let id = self.next_id;
        self.next_id += 1;
        self.pairs
            .iter_mut()
            .find(|pair| pair.id == pair_id)?
            .notes
            .push(PairNote {
                id,
                text: String::new(),
            });
        Some(id)
    }

    pub fn set_note(&mut self, pair_id: u64, note_id: u64, text: &str) -> bool {
        let Some(pair) = self.pairs.iter_mut().find(|pair| pair.id == pair_id) else {
            return false;
        };
        let Some(note) = pair.notes.iter_mut().find(|note| note.id == note_id) else {
            return false;
        };
        note.text = text.to_string();
        true
    }

    pub fn delete_note(&mut self, pair_id: u64, note_id: u64) -> bool {
        let Some(pair) = self.pairs.iter_mut().find(|pair| pair.id == pair_id) else {
            return false;
        };
        let before = pair.notes.len();
        pair.notes.retain(|note| note.id != note_id);
        pair.notes.len() != before
    }

    pub fn add_pin(&mut self, pair_id: u64, x: f32, y: f32, w: f32, h: f32) -> Option<u64> {
        let id = self.next_id;
        self.next_id += 1;
        self.pairs
            .iter_mut()
            .find(|pair| pair.id == pair_id)?
            .pins
            .push(Pin {
                id,
                title: String::new(),
                text: String::new(),
                x: x.clamp(0.0, 1.0),
                y: y.clamp(0.0, 1.0),
                w: w.clamp(0.0, 1.0),
                h: h.clamp(0.0, 1.0),
            });
        Some(id)
    }

    pub fn set_pin_text(&mut self, pair_id: u64, pin_id: u64, title: &str, text: &str) -> bool {
        let Some(pin) = self.pin_mut(pair_id, pin_id) else {
            return false;
        };
        pin.title = title.to_string();
        pin.text = text.to_string();
        true
    }

    pub fn set_pin_bounds(&mut self, pair_id: u64, pin_id: u64, x: f32, y: f32, w: f32, h: f32) -> bool {
        let Some(pin) = self.pin_mut(pair_id, pin_id) else {
            return false;
        };
        pin.x = x.clamp(0.0, 1.0);
        pin.y = y.clamp(0.0, 1.0);
        pin.w = w.clamp(0.0, 1.0);
        pin.h = h.clamp(0.0, 1.0);
        true
    }

    pub fn delete_pin(&mut self, pair_id: u64, pin_id: u64) -> bool {
        let Some(pair) = self.pairs.iter_mut().find(|pair| pair.id == pair_id) else {
            return false;
        };
        let before = pair.pins.len();
        pair.pins.retain(|pin| pin.id != pin_id);
        pair.pins.len() != before
    }

    fn pin_mut(&mut self, pair_id: u64, pin_id: u64) -> Option<&mut Pin> {
        self.pairs
            .iter_mut()
            .find(|pair| pair.id == pair_id)?
            .pins
            .iter_mut()
            .find(|pin| pin.id == pin_id)
    }

    pub fn custom_order(&self) -> bool {
        self.custom_order
    }

    pub fn order(&self) -> PairOrder {
        self.order
    }

    /// Move one visible pair into another slot of the same visible subset.
    /// Hidden pairs stay where they are. `from` and `to` are indexes into `visible`.
    pub fn reorder_visible(&mut self, visible: &[usize], from: usize, to: usize) -> bool {
        if from == to || from >= visible.len() || to >= visible.len() {
            return false;
        }
        if visible.iter().any(|index| *index >= self.pairs.len()) {
            return false;
        }
        let mut items: Vec<Pair> = visible.iter().map(|index| self.pairs[*index].clone()).collect();
        let moved = items.remove(from);
        items.insert(to, moved);
        for (slot, item) in visible.iter().zip(items) {
            self.pairs[*slot] = item;
        }
        self.custom_order = true;
        self.order = PairOrder::Custom;
        true
    }

    /// Sort the pairs in `visible` and write them back into those same slots.
    /// Custom keeps the current order and remembers that dragging owns it.
    pub fn sort_visible(&mut self, visible: &[usize], order: PairOrder) -> bool {
        if visible.iter().any(|index| *index >= self.pairs.len()) {
            return false;
        }
        self.order = order;
        self.custom_order = order == PairOrder::Custom;
        if order == PairOrder::Custom || visible.len() < 2 {
            return true;
        }
        let mut items: Vec<Pair> = visible.iter().map(|index| self.pairs[*index].clone()).collect();
        items.sort_by(|left, right| pair_rank(left, right, order));
        for (slot, item) in visible.iter().zip(items) {
            self.pairs[*slot] = item;
        }
        true
    }

    fn keep_named_order(&mut self) {
        if self.order == PairOrder::Custom {
            return;
        }
        let order = self.order;
        let visible: Vec<usize> = (0..self.pairs.len()).collect();
        self.sort_visible(&visible, order);
    }

    /// Append valid before/after photo ids in the given order.
    pub fn commit_matched(&mut self, pairs: &[(u64, u64)]) -> usize {
        let mut created = 0;
        for (before, after) in pairs {
            if self.pair_index(*before).is_some() || self.pair_index(*after).is_some() {
                continue;
            }
            self.draft_before = Some(*before);
            self.draft_after = Some(*after);
            if self.commit_draft("").is_some() {
                created += 1;
            }
        }
        created
    }

    /// Create a named group from complete pairs. Unknown ids are ignored.
    /// A pair may belong to more than one group. An empty selection is refused.
    pub fn create_group(&mut self, name: &str, pair_ids: &[u64]) -> Option<u64> {
        let mut chosen = Vec::new();
        for id in pair_ids {
            if self.pair_by_id(*id).is_some() && !chosen.contains(id) {
                chosen.push(*id);
            }
        }
        if chosen.is_empty() {
            return None;
        }
        let trimmed = name.trim();
        let name = if trimmed.is_empty() {
            "Untitled".to_string()
        } else {
            trimmed.to_string()
        };
        let id = self.next_id;
        self.next_id += 1;
        self.groups.push(Group {
            id,
            name,
            pairs: chosen,
        });
        Some(id)
    }

    /// Pair indexes shown for a viewer filter.
    /// An empty group list means every pair. Combined groups list each pair once, in library order.
    pub fn visible_pairs(&self, group_ids: &[u64]) -> Vec<usize> {
        if group_ids.is_empty() {
            return (0..self.pairs.len()).collect();
        }
        let mut allowed = Vec::new();
        for group_id in group_ids {
            if let Some(group) = self.group(*group_id) {
                for pair_id in &group.pairs {
                    if !allowed.contains(pair_id) {
                        allowed.push(*pair_id);
                    }
                }
            }
        }
        self.pairs
            .iter()
            .enumerate()
            .filter(|(_, pair)| allowed.contains(&pair.id))
            .map(|(index, _)| index)
            .collect()
    }

    pub fn unlink(&mut self, index: usize) -> bool {
        let Some(pair) = self.pairs.get(index) else {
            return false;
        };
        let id = pair.id;
        self.pairs.remove(index);
        for group in &mut self.groups {
            group.pairs.retain(|pair_id| *pair_id != id);
        }
        true
    }

    /// Remove one photo that is not in a pair. Files on disk are left in place.
    pub fn take_loose(&mut self, id: u64) -> Option<Photo> {
        if !self.is_loose(id) {
            return None;
        }
        self.extract_photo(id)
    }

    /// Remove a pair and both of its photos. Group lists drop that pair.
    /// Returns the stored photos. Files on disk are left in place.
    pub fn take_pair(&mut self, index: usize) -> Option<Vec<Photo>> {
        if index >= self.pairs.len() {
            return None;
        }
        let pair = self.pairs.remove(index);
        for group in &mut self.groups {
            group.pairs.retain(|pair_id| *pair_id != pair.id);
        }
        let mut removed = Vec::new();
        if let Some(photo) = self.extract_photo(pair.before) {
            removed.push(photo);
        }
        if pair.after != pair.before {
            if let Some(photo) = self.extract_photo(pair.after) {
                removed.push(photo);
            }
        }
        Some(removed)
    }

    fn extract_photo(&mut self, id: u64) -> Option<Photo> {
        let index = self.photos.iter().position(|photo| photo.id == id)?;
        if self.draft_before == Some(id) {
            self.draft_before = None;
        }
        if self.draft_after == Some(id) {
            self.draft_after = None;
        }
        Some(self.photos.remove(index))
    }

    pub fn set_split(&mut self, index: usize, split: f32) -> bool {
        let Some(pair) = self.pairs.get_mut(index) else {
            return false;
        };
        let split = split.clamp(SPLIT_MIN, SPLIT_MAX);
        if (pair.split - split).abs() < 0.0005 {
            return false;
        }
        pair.split = split;
        true
    }

    pub fn nudge_split(&mut self, index: usize, delta: f32) -> bool {
        let Some(pair) = self.pairs.get(index) else {
            return false;
        };
        let split = pair.split + delta;
        self.set_split(index, split)
    }
}

pub fn step_index(index: usize, len: usize, delta: i32) -> usize {
    if len == 0 {
        return 0;
    }
    let last = (len - 1) as i32;
    (index as i32 + delta).clamp(0, last) as usize
}

pub fn file_label(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("photo")
        .to_string()
}

/// "overview-before.png" becomes "Overview · before".
pub fn pretty_caption(path: &Path) -> String {
    let (title, role) = caption_parts(path);
    match role {
        Some(role) => format!("{title} · {role}"),
        None => title,
    }
}

/// Title used when a pair is created without a typed name.
pub fn suggest_name(path: &Path) -> String {
    let (title, _) = caption_parts(path);
    if title.is_empty() {
        "Untitled".to_string()
    } else {
        title
    }
}

fn caption_parts(path: &Path) -> (String, Option<String>) {
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("photo");
    let tokens: Vec<&str> = stem
        .split(|ch: char| ch == '-' || ch == '_' || ch == ' ')
        .filter(|token| !token.is_empty())
        .collect();
    if tokens.len() >= 2 {
        let last = tokens[tokens.len() - 1].to_ascii_lowercase();
        if last == "before" || last == "after" {
            return (title_case(&tokens[..tokens.len() - 1]), Some(last));
        }
    }
    (title_case(&tokens), None)
}

fn title_case(tokens: &[&str]) -> String {
    tokens
        .iter()
        .map(|token| {
            let mut chars = token.chars();
            match chars.next() {
                Some(first) => {
                    let mut word = first.to_uppercase().collect::<String>();
                    word.push_str(&chars.as_str().to_lowercase());
                    word
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Expand directories, copy decodable images into local storage, and turn HEIC/HEIF into JPEG.
/// Each item is the stored path and the original filename.
pub fn prepare_paths(paths: Vec<PathBuf>) -> Vec<(PathBuf, String)> {
    let mut found = Vec::new();
    for path in paths {
        collect(&path, 0, &mut found);
    }
    found
        .into_iter()
        .filter_map(|path| archive_image(&path))
        .collect()
}

/// Copy one image into the local asset directory. The original file stays where it is.
pub fn archive_image(source: &Path) -> Option<(PathBuf, String)> {
    let label = file_label(source);
    let bytes = if is_image(source) {
        fs::read(source).ok()?
    } else if is_heic(source) {
        let jpeg = transcode_heic(source)?;
        let bytes = fs::read(&jpeg).ok()?;
        let _ = fs::remove_file(&jpeg);
        bytes
    } else {
        return None;
    };
    let ext = if is_heic(source) {
        "jpg".to_string()
    } else {
        extension(source)?
    };
    let path = write_asset(&bytes, &ext).ok()?;
    // A missing thumbnail must not reject the import. Lists draw a placeholder until one exists.
    let _ = write_thumbnail(&path);
    Some((path, label))
}

fn collect(path: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 6 {
        return;
    }
    let Ok(meta) = fs::symlink_metadata(path) else {
        return;
    };
    if meta.file_type().is_symlink() {
        return;
    }
    if meta.is_dir() {
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            collect(&entry.path(), depth + 1, out);
        }
    } else if meta.is_file() {
        out.push(path.to_path_buf());
    }
}

fn is_image(path: &Path) -> bool {
    matches!(
        extension(path).as_deref(),
        Some("png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "tif" | "tiff")
    )
}

fn is_heic(path: &Path) -> bool {
    matches!(extension(path).as_deref(), Some("heic" | "heif"))
}

fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
}

fn transcode_heic(path: &Path) -> Option<PathBuf> {
    let directory = std::env::temp_dir().join("split-cache");
    fs::create_dir_all(&directory).ok()?;
    let out = directory.join(format!("{}.jpg", cache_key(path)));
    if out.is_file() {
        return Some(out);
    }
    let input = path.to_str()?;
    let output = out.to_str()?;
    let status = Command::new("sips")
        .args(["-s", "format", "jpeg", input, "--out", output])
        .status()
        .ok()?;
    if status.success() && out.is_file() {
        Some(out)
    } else {
        None
    }
}

fn cache_key(path: &Path) -> String {
    let meta = fs::metadata(path).ok();
    let modified = meta
        .as_ref()
        .and_then(|meta| meta.modified().ok())
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|time| time.as_secs())
        .unwrap_or(0);
    let len = meta.map(|meta| meta.len()).unwrap_or(0);
    format!("{:016x}-{len:x}-{modified:x}", fnv(path.to_string_lossy().as_bytes()))
}

fn fnv(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

pub fn photo_label(photo: &Photo) -> String {
    if photo.label.is_empty() {
        file_label(&photo.path)
    } else {
        photo.label.clone()
    }
}

pub fn source_label(photo: &Photo) -> String {
    match photo.source {
        PhotoSource::Clipboard => "Clipboard image".to_string(),
        PhotoSource::File => photo_label(photo),
    }
}

pub fn pair_position(library: &Library, id: u64) -> Option<usize> {
    library.pairs.iter().position(|pair| pair.id == id)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NumberPattern {
    pub prefix: String,
    pub suffix: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NumberHit {
    pub number: u64,
    pub context: String,
    pub digit_start: usize,
    pub digit_end: usize,
}

pub fn parse_number_pattern(pattern: &str) -> Result<NumberPattern, &'static str> {
    if pattern.matches("{n}").count() != 1 {
        return Err("Use exactly one {n}.");
    }
    let (prefix, suffix) = pattern.split_once("{n}").unwrap();
    Ok(NumberPattern {
        prefix: prefix.to_string(),
        suffix: suffix.to_string(),
    })
}

/// Match one `{n}` pattern against a filename stem.
/// `{n}` consumes one complete digit run, so `1` is not taken from `10`.
pub fn match_number(stem: &str, pattern: &NumberPattern) -> Option<NumberHit> {
    if pattern.prefix.is_empty() && pattern.suffix.is_empty() {
        if !stem.is_empty() && stem.bytes().all(|byte| byte.is_ascii_digit()) {
            return Some(NumberHit {
                number: stem.parse().ok()?,
                context: String::new(),
                digit_start: 0,
                digit_end: stem.len(),
            });
        }
        return None;
    }
    let mut search = 0;
    while search < stem.len() {
        let Some(relative) = stem[search..].find(&pattern.prefix) else {
            break;
        };
        let start = search + relative;
        let after_prefix = start + pattern.prefix.len();
        let digit_len = stem[after_prefix..]
            .bytes()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        if digit_len == 0 {
            search = start + 1;
            continue;
        }
        let after_digits = after_prefix + digit_len;
        if stem[after_digits..].starts_with(&pattern.suffix) {
            let after_suffix = after_digits + pattern.suffix.len();
            let digits = &stem[after_prefix..after_digits];
            return Some(NumberHit {
                number: digits.parse().ok()?,
                context: format!("{}{}", &stem[..start], &stem[after_suffix..]),
                digit_start: after_prefix,
                digit_end: after_digits,
            });
        }
        search = start + 1;
    }
    None
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Proposal {
    Ready {
        number: u64,
        context: String,
        before: u64,
        after: u64,
        before_digits: (usize, usize),
        after_digits: (usize, usize),
    },
    Missing {
        number: u64,
        context: String,
        before: Option<u64>,
        after: Option<u64>,
    },
    Ambiguous {
        number: u64,
        context: String,
    },
    Conflict {
        photo: u64,
    },
}

impl Proposal {
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready { .. })
    }
}

/// Pair unpaired files by a shared number and the filename text around each pattern.
/// `files` are `(id, filename including extension)`.
pub fn propose_numbered(
    files: &[(u64, &str)],
    before_pattern: &str,
    after_pattern: &str,
) -> Result<Vec<Proposal>, &'static str> {
    let before_pattern = parse_number_pattern(before_pattern)?;
    let after_pattern = parse_number_pattern(after_pattern)?;
    let mut conflicts = Vec::new();
    let mut buckets: Vec<Bucket> = Vec::new();

    for (id, filename) in files {
        let stem = filename
            .rsplit_once('.')
            .map(|(stem, _)| stem)
            .unwrap_or(filename);
        let before = match_number(stem, &before_pattern);
        let after = match_number(stem, &after_pattern);
        match (before, after) {
            (Some(_), Some(_)) => conflicts.push(*id),
            (Some(hit), None) => push_bucket(&mut buckets, *id, hit, true),
            (None, Some(hit)) => push_bucket(&mut buckets, *id, hit, false),
            (None, None) => {}
        }
    }

    buckets.sort_by(|left, right| {
        left.number
            .cmp(&right.number)
            .then(left.context.cmp(&right.context))
    });
    conflicts.sort_unstable();

    let mut proposals = Vec::new();
    for bucket in buckets {
        let one_before = bucket.befores.len() == 1;
        let one_after = bucket.afters.len() == 1;
        if one_before && one_after {
            proposals.push(Proposal::Ready {
                number: bucket.number,
                context: bucket.context,
                before: bucket.befores[0].0,
                after: bucket.afters[0].0,
                before_digits: bucket.befores[0].1,
                after_digits: bucket.afters[0].1,
            });
        } else if bucket.befores.len() > 1 || bucket.afters.len() > 1 {
            proposals.push(Proposal::Ambiguous {
                number: bucket.number,
                context: bucket.context,
            });
        } else {
            proposals.push(Proposal::Missing {
                number: bucket.number,
                context: bucket.context,
                before: bucket.befores.first().map(|(id, _)| *id),
                after: bucket.afters.first().map(|(id, _)| *id),
            });
        }
    }
    for photo in conflicts {
        proposals.push(Proposal::Conflict { photo });
    }
    Ok(proposals)
}

struct Bucket {
    number: u64,
    context: String,
    befores: Vec<(u64, (usize, usize))>,
    afters: Vec<(u64, (usize, usize))>,
}

fn push_bucket(buckets: &mut Vec<Bucket>, id: u64, hit: NumberHit, before: bool) {
    let digits = (hit.digit_start, hit.digit_end);
    if let Some(bucket) = buckets
        .iter_mut()
        .find(|bucket| bucket.number == hit.number && bucket.context == hit.context)
    {
        if before {
            bucket.befores.push((id, digits));
        } else {
            bucket.afters.push((id, digits));
        }
        return;
    }
    buckets.push(Bucket {
        number: hit.number,
        context: hit.context,
        befores: if before { vec![(id, digits)] } else { Vec::new() },
        afters: if before { Vec::new() } else { vec![(id, digits)] },
    });
}

pub fn ready_pairs(proposals: &[Proposal]) -> Vec<(u64, u64)> {
    proposals
        .iter()
        .filter_map(|proposal| match proposal {
            Proposal::Ready { before, after, .. } => Some((*before, *after)),
            _ => None,
        })
        .collect()
}

/// Case-insensitive highlights. Offsets follow the original text.
pub fn highlight_spans(text: &str, query: &str) -> Vec<(bool, String)> {
    let query = query.trim();
    if query.is_empty() {
        return vec![(false, text.to_string())];
    }
    let chars: Vec<char> = text.chars().collect();
    let folded: Vec<char> = chars
        .iter()
        .map(|ch| ch.to_lowercase().next().unwrap_or(*ch))
        .collect();
    let needle: Vec<char> = query
        .chars()
        .map(|ch| ch.to_lowercase().next().unwrap_or(ch))
        .collect();
    if needle.is_empty() {
        return vec![(false, text.to_string())];
    }
    let mut spans = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        if folded[index..].starts_with(&needle) {
            spans.push((true, needle_text(&chars, index, needle.len())));
            index += needle.len();
        } else {
            let start = index;
            index += 1;
            while index < chars.len() && !folded[index..].starts_with(&needle) {
                index += 1;
            }
            spans.push((false, needle_text(&chars, start, index - start)));
        }
    }
    spans
}

fn needle_text(chars: &[char], start: usize, len: usize) -> String {
    chars[start..start + len].iter().collect()
}

/// Search titles and filenames. `rows` are already in workspace order.
/// Each row is `(pair index, title, before filename, after filename)`.
pub fn search_pairs(rows: &[(usize, &str, &str, &str)], query: &str) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    rows.iter()
        .filter(|(_, title, before, after)| {
            query.is_empty()
                || title.to_lowercase().contains(&query)
                || before.to_lowercase().contains(&query)
                || after.to_lowercase().contains(&query)
        })
        .map(|(index, _, _, _)| *index)
        .collect()
}

/// Which pair and group filters were open. Pair and group ids, not list positions.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ViewState {
    pub focus_pair: Option<u64>,
    pub library_group: Option<u64>,
    pub viewer_groups: Vec<u64>,
}

#[derive(Serialize, Deserialize)]
struct WorkspaceFile {
    next_id: u64,
    photos: Vec<Photo>,
    pairs: Vec<Pair>,
    groups: Vec<Group>,
    #[serde(default)]
    custom_order: bool,
    #[serde(default)]
    order: Option<PairOrder>,
    #[serde(default)]
    compare_mode: CompareMode,
    #[serde(default = "default_blend")]
    blend: f32,
    #[serde(default)]
    fit_fill: bool,
    #[serde(default)]
    focus_pair: Option<u64>,
    #[serde(default)]
    library_group: Option<u64>,
    #[serde(default)]
    viewer_groups: Vec<u64>,
}

fn default_blend() -> f32 {
    0.5
}

pub fn app_support_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("COMPARE_HOME") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    #[cfg(target_os = "macos")]
    {
        PathBuf::from(home).join("Library/Application Support/Compare")
    }
    #[cfg(not(target_os = "macos"))]
    {
        PathBuf::from(home).join(".local/share/Compare")
    }
}

pub fn workspace_path() -> PathBuf {
    app_support_dir().join("workspace.json")
}

pub fn write_asset(bytes: &[u8], extension: &str) -> std::io::Result<PathBuf> {
    let dir = app_support_dir().join("assets");
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{:016x}.{}", fnv(bytes), extension));
    if !path.exists() {
        let mut file = fs::File::create(&path)?;
        file.write_all(bytes)?;
    }
    Ok(path)
}

fn thumb_path(asset: &Path) -> PathBuf {
    let stem = asset
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("image");
    app_support_dir().join("thumbs").join(format!("{stem}.jpg"))
}

/// Drop a file that lives in this app's asset or thumbnail store.
/// A path outside that store, including the file the user imported, is left alone.
pub fn discard_stored_copy(path: &Path) {
    let root = app_support_dir();
    if !path.starts_with(&root) {
        return;
    }
    let _ = fs::remove_file(path);
    let _ = fs::remove_file(thumb_path(path));
}

/// JPEG thumbnail for a stored asset, when one has already been written.
pub fn ready_thumb(asset: &Path) -> Option<PathBuf> {
    let path = thumb_path(asset);
    let meta = fs::metadata(&path).ok()?;
    if meta.is_file() && meta.len() > 0 {
        Some(path)
    } else {
        None
    }
}

/// Write a JPEG whose longest edge is at most 480 pixels.
/// Returns the existing file when one is already there. Failure deletes a partial file.
pub fn write_thumbnail(asset: &Path) -> Option<PathBuf> {
    if let Some(existing) = ready_thumb(asset) {
        return Some(existing);
    }
    let dest = thumb_path(asset);
    let dir = dest.parent()?;
    fs::create_dir_all(dir).ok()?;
    let input = asset.to_str()?;
    let output = dest.to_str()?;
    let status = Command::new("sips")
        .args(["-s", "format", "jpeg", "-Z", "480", input, "--out", output])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok();
    if status.is_some_and(|status| status.success()) {
        ready_thumb(asset)
    } else {
        let _ = fs::remove_file(&dest);
        None
    }
}

pub struct StoredWorkspace {
    pub library: Library,
    pub compare_mode: CompareMode,
    pub blend: f32,
    pub fit_fill: bool,
    pub view: ViewState,
}

impl Library {
    pub fn save_to(
        &self,
        path: &Path,
        compare_mode: CompareMode,
        blend: f32,
        fit_fill: bool,
        view: &ViewState,
    ) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let file = WorkspaceFile {
            next_id: self.next_id,
            photos: self.photos.clone(),
            pairs: self.pairs.clone(),
            groups: self.groups.clone(),
            custom_order: self.order == PairOrder::Custom,
            order: Some(self.order),
            compare_mode,
            blend,
            fit_fill,
            focus_pair: view.focus_pair,
            library_group: view.library_group,
            viewer_groups: view.viewer_groups.clone(),
        };
        let body = serde_json::to_vec_pretty(&file).map_err(|error| error.to_string())?;
        let temporary = path.with_extension("json.tmp");
        {
            let mut handle = fs::File::create(&temporary).map_err(|error| error.to_string())?;
            handle.write_all(&body).map_err(|error| error.to_string())?;
        }
        fs::rename(&temporary, path).map_err(|error| error.to_string())
    }

    pub fn load_from(path: &Path) -> Result<StoredWorkspace, String> {
        let body = fs::read(path).map_err(|error| error.to_string())?;
        let file: WorkspaceFile = serde_json::from_slice(&body).map_err(|error| error.to_string())?;
        let order = file.order.unwrap_or(if file.custom_order {
            PairOrder::Custom
        } else {
            PairOrder::Oldest
        });
        Ok(StoredWorkspace {
            library: Library {
                photos: file.photos,
                pairs: file.pairs,
                groups: file.groups,
                draft_before: None,
                draft_after: None,
                next_id: file.next_id,
                custom_order: order == PairOrder::Custom,
                order,
            },
            compare_mode: file.compare_mode,
            blend: file.blend.clamp(0.0, 1.0),
            fit_fill: file.fit_fill,
            view: ViewState {
                focus_pair: file.focus_pair,
                library_group: file.library_group,
                viewer_groups: file.viewer_groups,
            },
        })
    }

    pub fn save_app(&self, compare_mode: CompareMode, blend: f32, fit_fill: bool, view: &ViewState) {
        let _ = self.save_to(&workspace_path(), compare_mode, blend, fit_fill, view);
    }

    pub fn load_app() -> Option<StoredWorkspace> {
        Self::load_from(&workspace_path()).ok()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    static COMPARE_HOME_LOCK: Mutex<()> = Mutex::new(());

    fn sample(library: &mut Library, name: &str) -> u64 {
        library.insert(PathBuf::from(name)).expect("image")
    }

    fn linked(library: &mut Library, before: &str, after: &str, name: &str) -> u64 {
        let before = sample(library, before);
        let after = sample(library, after);
        library.select_for_draft(before);
        library.select_for_draft(after);
        library.commit_draft(name);
        library.pairs.last().unwrap().id
    }

    #[test]
    fn links_before_then_after_and_keeps_order() {
        let mut library = Library::new();
        let before = sample(&mut library, "x1.jpg");
        let after = sample(&mut library, "y1.jpg");
        let next_before = sample(&mut library, "x2.png");
        let next_after = sample(&mut library, "y2.png");

        library.select_for_draft(before);
        library.select_for_draft(after);
        assert_eq!(library.draft_before, Some(before));
        assert_eq!(library.draft_after, Some(after));
        assert_eq!(library.commit_draft("Overview"), Some(0));
        assert!(library.draft_before.is_none());
        assert_eq!(library.pairs[0].before, before);
        assert_eq!(library.pairs[0].after, after);
        assert_eq!(library.pairs[0].name, "Overview");
        assert!((library.pairs[0].split - 0.5).abs() < f32::EPSILON);

        library.select_for_draft(next_before);
        library.select_for_draft(next_after);
        library.commit_draft("");
        assert_eq!(library.pairs.len(), 2);
        assert!(library.unassigned().is_empty());
        assert_eq!(library.pairs[1].before, next_before);
        assert_eq!(library.pairs[1].after, next_after);
        assert_eq!(library.pairs[1].name, "X2");
    }

    #[test]
    fn clicking_a_drafted_photo_clears_that_side() {
        let mut library = Library::new();
        let before = sample(&mut library, "only.jpg");
        let after = sample(&mut library, "other.jpg");
        let third = sample(&mut library, "third.jpg");
        library.select_for_draft(before);
        library.select_for_draft(before);
        assert!(library.draft_before.is_none());
        assert!(library.pairs.is_empty());

        library.select_for_draft(before);
        library.select_for_draft(after);
        library.select_for_draft(third);
        assert_eq!(library.draft_before, Some(before));
        assert_eq!(library.draft_after, Some(third));
        library.select_for_draft(third);
        assert_eq!(library.draft_after, None);
        assert_eq!(library.unassigned().len(), 3);
    }

    #[test]
    fn unlink_returns_photos_to_the_pool_in_upload_order() {
        let mut library = Library::new();
        let ids: Vec<_> = ["a.jpg", "b.jpg", "c.jpg", "d.webp"]
            .into_iter()
            .map(|name| sample(&mut library, name))
            .collect();
        library.select_for_draft(ids[0]);
        library.select_for_draft(ids[2]);
        library.commit_draft("One");
        library.select_for_draft(ids[1]);
        library.select_for_draft(ids[3]);
        library.commit_draft("Two");
        assert!(library.unlink(0));
        let loose: Vec<_> = library.unassigned().iter().map(|photo| photo.id).collect();
        assert_eq!(loose, vec![ids[0], ids[2]]);
        assert_eq!(library.pairs.len(), 1);
        assert_eq!(library.pairs[0].before, ids[1]);
    }

    #[test]
    fn take_loose_removes_one_unpaired_photo_and_refuses_a_paired_one() {
        let mut library = Library::new();
        let loose = sample(&mut library, "loose.jpg");
        let before = sample(&mut library, "before.jpg");
        let after = sample(&mut library, "after.jpg");
        library.select_for_draft(before);
        library.select_for_draft(after);
        library.commit_draft("Kept");
        library.select_for_draft(loose);

        assert!(library.take_loose(before).is_none());
        let removed = library.take_loose(loose).expect("loose photo");
        assert_eq!(removed.id, loose);
        assert!(library.photo(loose).is_none());
        assert!(library.draft_before.is_none());
        assert_eq!(library.photos().len(), 2);
        assert_eq!(library.pairs.len(), 1);
    }

    #[test]
    fn take_pair_removes_the_pair_its_photos_and_the_group_membership() {
        let mut library = Library::new();
        let kept = linked(&mut library, "kept-before.jpg", "kept-after.jpg", "Kept");
        let gone = linked(&mut library, "gone-before.jpg", "gone-after.jpg", "Gone");
        let loose = sample(&mut library, "still.jpg");
        let group = library.create_group("Scenes", &[kept, gone]).unwrap();

        let removed = library.take_pair(1).expect("pair");
        assert_eq!(removed.len(), 2);
        assert!(library.pair_by_id(gone).is_none());
        assert_eq!(library.pairs.len(), 1);
        assert_eq!(library.pairs[0].id, kept);
        assert!(library.unassigned().iter().all(|photo| photo.id == loose));
        assert_eq!(library.group(group).unwrap().pair_ids(), &[kept]);
        assert!(library.take_pair(4).is_none());
    }

    #[test]
    fn discard_stored_copy_removes_library_copies_and_leaves_originals() {
        let _home = COMPARE_HOME_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("compare-discard-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let home = dir.join("home");
        fs::create_dir_all(&home).unwrap();
        let previous = std::env::var("COMPARE_HOME").ok();
        unsafe { std::env::set_var("COMPARE_HOME", &home) };

        let asset_dir = home.join("assets");
        let thumb_dir = home.join("thumbs");
        fs::create_dir_all(&asset_dir).unwrap();
        fs::create_dir_all(&thumb_dir).unwrap();
        let asset = asset_dir.join("abc.jpg");
        let thumb = thumb_dir.join("abc.jpg");
        let original = dir.join("original.jpg");
        fs::write(&asset, b"copy").unwrap();
        fs::write(&thumb, b"thumb").unwrap();
        fs::write(&original, b"original").unwrap();

        discard_stored_copy(&original);
        assert!(original.is_file());
        assert!(asset.is_file());
        discard_stored_copy(&asset);
        assert!(!asset.exists());
        assert!(!thumb.exists());
        assert!(original.is_file());

        unsafe {
            match previous {
                Some(value) => std::env::set_var("COMPARE_HOME", value),
                None => std::env::remove_var("COMPARE_HOME"),
            }
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn duplicates_and_unknown_types_are_skipped() {
        let mut library = Library::new();
        assert!(library.insert(PathBuf::from("notes.txt")).is_none());
        let id = sample(&mut library, "one.jpeg");
        assert!(library.insert(PathBuf::from("one.jpeg")).is_none());
        assert_eq!(library.photos().len(), 1);
        assert_eq!(library.photo(id).unwrap().path, PathBuf::from("one.jpeg"));
    }

    #[test]
    fn step_clamps_at_the_ends() {
        assert_eq!(step_index(0, 3, -1), 0);
        assert_eq!(step_index(0, 3, 1), 1);
        assert_eq!(step_index(2, 3, 1), 2);
        assert_eq!(step_index(1, 3, -1), 0);
        assert_eq!(step_index(0, 0, 1), 0);
    }

    #[test]
    fn split_stays_inside_the_frame() {
        let mut library = Library::new();
        let before = sample(&mut library, "b.jpg");
        let after = sample(&mut library, "a.jpg");
        library.select_for_draft(before);
        library.select_for_draft(after);
        library.commit_draft("Pair");
        assert!(!library.set_split(0, 0.5));
        assert!(library.set_split(0, 0.0));
        assert!((library.pairs[0].split - SPLIT_MIN).abs() < 0.001);
        assert!(library.nudge_split(0, 1.0));
        assert!((library.pairs[0].split - SPLIT_MAX).abs() < 0.001);
    }

    #[test]
    fn combined_groups_list_each_pair_once() {
        let mut library = Library::new();
        let mountains = linked(&mut library, "mountains-before.jpg", "mountains-after.jpg", "Mountains");
        let house = linked(&mut library, "house-before.jpg", "house-after.jpg", "House");
        let desk = linked(&mut library, "desk-before.jpg", "desk-after.jpg", "Desk");
        let notes = linked(&mut library, "notes-before.jpg", "notes-after.jpg", "Notes");

        let scenes = library
            .create_group("Scenes", &[mountains, house, desk])
            .unwrap();
        let review = library.create_group("Review", &[house, notes, house]).unwrap();
        assert!(library.create_group("Empty", &[]).is_none());
        assert!(library.create_group("Missing", &[999]).is_none());
        assert_eq!(library.group(scenes).unwrap().pair_count(), 3);
        assert_eq!(library.group(review).unwrap().pair_count(), 2);
        assert_eq!(library.group_label(house), "Scenes · Review");
        assert_eq!(library.visible_pairs(&[]), vec![0, 1, 2, 3]);
        assert_eq!(library.visible_pairs(&[scenes, review]), vec![0, 1, 2, 3]);
        assert_eq!(library.visible_pairs(&[review]), vec![1, 3]);

        let house_index = library
            .pairs
            .iter()
            .position(|pair| pair.id == house)
            .unwrap();
        assert!(library.unlink(house_index));
        assert_eq!(library.group(scenes).unwrap().pair_count(), 2);
        assert_eq!(library.group(review).unwrap().pair_count(), 1);
        assert!(!library.group(review).unwrap().pair_ids().contains(&house));
        assert_eq!(library.visible_pairs(&[scenes, review]).len(), 3);
    }

    #[test]
    fn captions_split_a_before_or_after_suffix() {
        assert_eq!(
            pretty_caption(Path::new("overview-before.png")),
            "Overview · before"
        );
        assert_eq!(suggest_name(Path::new("project-detail-after.jpg")), "Project Detail");
        assert_eq!(pretty_caption(Path::new("my_photo.webp")), "My Photo");
    }

    #[test]
    fn numbered_patterns_pair_complete_numbers_and_keep_context() {
        let files = [
            (1, "before_1.png"),
            (2, "after_1.png"),
            (3, "before_2.png"),
            (4, "after_2.png"),
            (5, "before_10.png"),
            (6, "after_10.png"),
            (7, "home_before_1.png"),
            (8, "home_after_1.png"),
            (9, "settings_before_1.png"),
            (10, "settings_after_1.png"),
            (11, "before_3.png"),
        ];
        let proposals = propose_numbered(&files, "before_{n}", "after_{n}").unwrap();
        let ready: Vec<_> = proposals
            .iter()
            .filter_map(|proposal| match proposal {
                Proposal::Ready {
                    number,
                    context,
                    before,
                    after,
                    ..
                } => Some((*number, context.clone(), *before, *after)),
                _ => None,
            })
            .collect();
        assert_eq!(
            ready,
            vec![
                (1, String::new(), 1, 2),
                (1, "home_".into(), 7, 8),
                (1, "settings_".into(), 9, 10),
                (2, String::new(), 3, 4),
                (10, String::new(), 5, 6),
            ]
        );
        assert!(proposals.iter().any(|proposal| matches!(
            proposal,
            Proposal::Missing {
                number: 3,
                before: Some(11),
                after: None,
                ..
            }
        )));
        assert!(propose_numbered(&files, "before_", "after_{n}").is_err());
    }

    #[test]
    fn duplicate_numbers_are_ambiguous_and_both_roles_conflict() {
        let files = [
            (1, "before_1.png"),
            (2, "before_1.jpg"),
            (3, "after_1.png"),
        ];
        let proposals = propose_numbered(&files, "before_{n}", "after_{n}").unwrap();
        assert!(proposals.iter().any(|proposal| matches!(
            proposal,
            Proposal::Ambiguous { number: 1, .. }
        )));
        let both = [(1, "1.png")];
        let conflict = propose_numbered(&both, "{n}", "{n}").unwrap();
        assert!(matches!(conflict[0], Proposal::Conflict { photo: 1 }));
    }

    #[test]
    fn filtered_reorder_keeps_hidden_pairs_and_notes() {
        let mut library = Library::new();
        let ids = ["a.jpg", "b.jpg", "c.jpg", "d.jpg", "e.jpg", "f.jpg"]
            .into_iter()
            .map(|name| sample(&mut library, name))
            .collect::<Vec<_>>();
        for pair in [(0, 1), (2, 3), (4, 5)] {
            library.select_for_draft(ids[pair.0]);
            library.select_for_draft(ids[pair.1]);
            library.commit_draft("");
        }
        let first = library.pairs[0].id;
        library.add_note(first);
        library.set_note(first, library.pairs[0].notes[0].id, "keep me");
        assert!(library.reorder_visible(&[0, 2], 1, 0));
        assert_eq!(
            library.pairs.iter().map(|pair| pair.before).collect::<Vec<_>>(),
            vec![ids[4], ids[2], ids[0]]
        );
        assert_eq!(library.pairs[2].notes[0].text, "keep me");
        assert!(library.custom_order());
        assert_eq!(library.order(), PairOrder::Custom);
        let visible = library.visible_pairs(&[]);
        assert_eq!(visible, vec![0, 1, 2]);
    }

    #[test]
    fn named_sort_orders_the_visible_slots_and_keeps_a_hidden_pair() {
        let mut library = Library::new();
        let ids = ["a.jpg", "b.jpg", "c.jpg", "d.jpg", "e.jpg", "f.jpg"]
            .into_iter()
            .map(|name| sample(&mut library, name))
            .collect::<Vec<_>>();
        for (name, pair) in [("C", (0, 1)), ("B", (2, 3)), ("A", (4, 5))] {
            library.select_for_draft(ids[pair.0]);
            library.select_for_draft(ids[pair.1]);
            library.commit_draft(name);
        }
        assert!(library.sort_visible(&[0, 2], PairOrder::Name));
        assert_eq!(
            library.pairs.iter().map(|pair| pair.name.as_str()).collect::<Vec<_>>(),
            vec!["A", "B", "C"]
        );
        assert_eq!(library.order(), PairOrder::Name);
        assert!(!library.custom_order());
        assert!(library.sort_visible(&[0, 1, 2], PairOrder::Oldest));
        assert_eq!(
            library.pairs.iter().map(|pair| pair.name.as_str()).collect::<Vec<_>>(),
            vec!["C", "B", "A"]
        );
    }

    #[test]
    fn editing_a_pair_keeps_its_identity() {
        let mut library = Library::new();
        let before = sample(&mut library, "notes-before.png");
        let after = sample(&mut library, "notes-after.png");
        let replacement = sample(&mut library, "pasted.png");
        library.select_for_draft(before);
        library.select_for_draft(after);
        library.commit_draft("Notes");
        let id = library.pairs[0].id;
        let note = library.add_note(id).unwrap();
        library.set_note(id, note, "Blue exterior");
        assert!(library.update_pair(id, "Notes", before, replacement));
        assert_eq!(library.pairs[0].id, id);
        assert_eq!(library.pairs[0].after, replacement);
        assert_eq!(library.pairs[0].notes[0].text, "Blue exterior");
    }

    #[test]
    fn workspace_round_trip_keeps_pins_order_and_mode() {
        let mut library = Library::new();
        let before = sample(&mut library, "house-before.png");
        let after = sample(&mut library, "house-after.png");
        library.select_for_draft(before);
        library.select_for_draft(after);
        library.commit_draft("House");
        let id = library.pairs[0].id;
        library.add_pin(id, 0.62, 0.48, 0.12, 0.16);
        library.set_pin_text(id, library.pairs[0].pins[0].id, "Right window", "Check contrast");
        library.reorder_visible(&[0], 0, 0);
        let path = std::env::temp_dir().join(format!("compare-test-{}.json", id));
        let view = ViewState {
            focus_pair: Some(id),
            library_group: None,
            viewer_groups: vec![7],
        };
        library
            .save_to(&path, CompareMode::Overlay, 0.5, false, &view)
            .unwrap();
        let loaded = Library::load_from(&path).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(loaded.compare_mode, CompareMode::Overlay);
        assert!((loaded.blend - 0.5).abs() < 0.001);
        assert_eq!(loaded.library.pairs[0].pins[0].title, "Right window");
        assert!((loaded.library.pairs[0].pins[0].x - 0.62).abs() < 0.001);
        assert_eq!(loaded.view.focus_pair, Some(id));
        assert_eq!(loaded.view.viewer_groups, vec![7]);
        assert_eq!(
            photo_label(&loaded.library.photos()[0]),
            "house-before.png"
        );
    }

    #[test]
    fn archived_upload_keeps_its_filename_after_reload() {
        let _home = COMPARE_HOME_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("compare-home-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let previous = std::env::var("COMPARE_HOME").ok();
        unsafe { std::env::set_var("COMPARE_HOME", &dir) };
        let source_dir = dir.join("incoming");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("before_1.png");
        fs::write(&source, b"png-bytes").unwrap();
        let archived = archive_image(&source).unwrap();
        assert_eq!(archived.1, "before_1.png");
        assert!(archived.0.starts_with(dir.join("assets")));
        assert!(archived.0.is_file());
        assert!(source.is_file());
        let again = archive_image(&source).unwrap();
        assert_eq!(again.0, archived.0);

        let mut library = Library::new();
        assert_eq!(library.add_prepared([archived.clone()]), 1);
        assert_eq!(library.add_prepared([archived.clone()]), 0);
        let before = library.photos()[0].id;
        let after_file = source_dir.join("after_1.png");
        fs::write(&after_file, b"other-png").unwrap();
        let after_archived = archive_image(&after_file).unwrap();
        library.add_prepared([after_archived]);
        let after = library.photos()[1].id;
        library.select_for_draft(before);
        library.select_for_draft(after);
        library.commit_draft("");
        let pair_id = library.pairs[0].id;
        let group = library.create_group("Scenes", &[pair_id]).unwrap();
        let saved = dir.join("workspace.json");
        library
            .save_to(
                &saved,
                CompareMode::Wipe,
                0.25,
                false,
                &ViewState {
                    focus_pair: Some(pair_id),
                    library_group: Some(group),
                    viewer_groups: vec![group],
                },
            )
            .unwrap();
        let loaded = Library::load_from(&saved).unwrap();
        assert_eq!(photo_label(&loaded.library.photos()[0]), "before_1.png");
        assert_eq!(photo_label(&loaded.library.photos()[1]), "after_1.png");
        assert!(loaded.library.photos()[0].path.is_file());
        assert_eq!(loaded.library.pairs[0].name, "Before 1");
        assert_eq!(loaded.library.group(group).unwrap().pair_count(), 1);
        assert_eq!(loaded.view.library_group, Some(group));
        unsafe {
            match previous {
                Some(value) => std::env::set_var("COMPARE_HOME", value),
                None => std::env::remove_var("COMPARE_HOME"),
            }
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn thumbnail_is_a_small_jpeg_and_a_bad_file_still_archives() {
        let _home = COMPARE_HOME_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("compare-thumbs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let previous = std::env::var("COMPARE_HOME").ok();
        unsafe { std::env::set_var("COMPARE_HOME", &dir) };

        let source_dir = dir.join("incoming");
        fs::create_dir_all(&source_dir).unwrap();
        let fake = source_dir.join("before_1.png");
        fs::write(&fake, b"png-bytes").unwrap();
        let archived = archive_image(&fake).unwrap();
        assert!(archived.0.is_file());
        assert!(ready_thumb(&archived.0).is_none());

        let real = source_dir.join("pixel.png");
        fs::write(&real, ONE_PX_PNG).unwrap();
        let stored = archive_image(&real).unwrap();
        let thumb = ready_thumb(&stored.0).expect("sips thumbnail");
        assert!(thumb.starts_with(dir.join("thumbs")));
        assert_eq!(thumb.extension().and_then(|ext| ext.to_str()), Some("jpg"));
        assert!(thumb.metadata().unwrap().len() > 0);
        assert_eq!(write_thumbnail(&stored.0).unwrap(), thumb);
        assert!(real.is_file());

        unsafe {
            match previous {
                Some(value) => std::env::set_var("COMPARE_HOME", value),
                None => std::env::remove_var("COMPARE_HOME"),
            }
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn search_matches_title_or_filename_once() {
        let rows = [
            (0, "House", "house-before.png", "house-after.png"),
            (2, "Notes", "notes-before.png", "notes-after.png"),
            (4, "Desk", "desk-before.png", "desk-after.png"),
        ];
        assert_eq!(search_pairs(&rows, "HOUSE"), vec![0]);
        assert_eq!(search_pairs(&rows, "before.png"), vec![0, 2, 4]);
        assert_eq!(search_pairs(&rows, "missing"), Vec::<usize>::new());
        let spans = highlight_spans("house-before.png", "house");
        assert_eq!(spans[0], (true, "house".into()));
    }

    const ONE_PX_PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8,
        0xCF, 0xC0, 0x00, 0x00, 0x00, 0x03, 0x00, 0x01, 0x00, 0x05, 0xFE, 0x02, 0xFE, 0xDC, 0xCC,
        0x59, 0xE7, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
}
