use crate::config::{Plan, RestorePlan, Scope, Store, extract_ssh_key, ssh_command_for_key};
use gpui::{
    App, Application, Bounds, ClipboardItem, Context, CursorStyle, Div, Element, ElementId,
    ElementInputHandler, Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable,
    FontWeight, GlobalElementId, Hsla, InteractiveElement, IntoElement, KeyBinding, LayoutId,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, ParentElement,
    PathBuilder, Pixels, Point, Render, SharedString, Stateful, StatefulInteractiveElement, Style,
    Styled, Subscription, TextRun, TitlebarOptions, UTF16Selection, UnderlineStyle, Window,
    WindowBounds, WindowOptions, actions, canvas, div, fill, point, prelude::*, px, relative, rgb,
    rgba, size,
};
use std::ops::Range;
use std::path::PathBuf;
use unicode_segmentation::UnicodeSegmentation;

const BG_DARK: u32 = 0x0f1218;
const BG_PANEL: u32 = 0x141820;
const BG_CARD: u32 = 0x1a1f29;
const BG_CARD_HOVER: u32 = 0x212734;
const BG_CARD_ACTIVE: u32 = 0x182a2e;
const BG_INPUT: u32 = 0x11151c;
const BORDER_SUBTLE: u32 = 0x2a3242;
const BORDER_ACTIVE: u32 = 0x64d8c8;
const TEXT_PRIMARY: u32 = 0xe8edf2;
const TEXT_MUTED: u32 = 0x8e9aad;
const ACCENT_TEAL: u32 = 0x64d8c8;
const ACCENT_TEAL_DARK: u32 = 0x164440;
const ACCENT_PURPLE: u32 = 0xb39cfb;
const ACCENT_PURPLE_DARK: u32 = 0x302652;
const WARN_AMBER: u32 = 0xf5b95f;
const WARN_BG: u32 = 0x443014;
const DANGER_RED: u32 = 0xf26e6e;
const DANGER_BG: u32 = 0x421a20;

const COMPACT_WINDOW_BREAKPOINT: f32 = 820.0;
const STACKED_SIDEBAR_BREAKPOINT: f32 = 640.0;
const TWO_COLUMN_BREAKPOINT: f32 = 540.0;
const THREE_COLUMN_BREAKPOINT: f32 = 620.0;

const AUTH_MODES: [(&str, &str); 3] = [
    ("Inherit", "Use global Git / gh credentials"),
    ("Personal Login", "Switch gh CLI user automatically"),
    ("GitHub App", "Mint short-lived installation tokens"),
];

const SIGNING_FORMATS: [&str; 2] = ["GPG (openpgp)", "SSH (ssh)"];

actions!(
    bipolar_desktop,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        ShowCharacterPalette,
        Paste,
        Cut,
        Copy,
        Tab,
        TabPrev,
        CancelModal,
        Quit,
    ]
);

pub fn run(store: Store) -> anyhow::Result<()> {
    Application::new().run(move |cx: &mut App| {
        bind_keys(cx);

        let bounds = Bounds::centered(None, size(px(1180.0), px(860.0)), cx);
        let store = store.clone();
        if let Ok(window) = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(400.0), px(420.0))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Bipolar".into()),
                    ..Default::default()
                }),
                app_id: Some("dev.iltumio.Bipolar".into()),
                focus: true,
                ..Default::default()
            },
            move |window, cx| {
                let app = Desktop::new(store);
                cx.new(|cx| DesktopView::new(app, window, cx))
            },
        ) {
            let _ = window.update(cx, |view, window, cx| {
                window.focus(&view.focus_handle(cx));
                cx.activate(true);
            });
        }

        cx.on_window_closed(|cx| {
            cx.quit();
        })
        .detach();
        cx.on_action(|_: &Quit, cx| cx.quit());
    });
    Ok(())
}

fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, Some("TextInput")),
        KeyBinding::new("delete", Delete, Some("TextInput")),
        KeyBinding::new("left", Left, Some("TextInput")),
        KeyBinding::new("right", Right, Some("TextInput")),
        KeyBinding::new("shift-left", SelectLeft, Some("TextInput")),
        KeyBinding::new("shift-right", SelectRight, Some("TextInput")),
        KeyBinding::new("ctrl-a", SelectAll, Some("TextInput")),
        KeyBinding::new("cmd-a", SelectAll, Some("TextInput")),
        KeyBinding::new("ctrl-v", Paste, Some("TextInput")),
        KeyBinding::new("cmd-v", Paste, Some("TextInput")),
        KeyBinding::new("ctrl-c", Copy, Some("TextInput")),
        KeyBinding::new("cmd-c", Copy, Some("TextInput")),
        KeyBinding::new("ctrl-x", Cut, Some("TextInput")),
        KeyBinding::new("cmd-x", Cut, Some("TextInput")),
        KeyBinding::new("home", Home, Some("TextInput")),
        KeyBinding::new("end", End, Some("TextInput")),
        KeyBinding::new(
            "ctrl-cmd-space",
            ShowCharacterPalette,
            Some("TextInput"),
        ),
        KeyBinding::new("tab", Tab, None),
        KeyBinding::new("shift-tab", TabPrev, None),
        KeyBinding::new("escape", CancelModal, None),
        KeyBinding::new("ctrl-q", Quit, None),
        KeyBinding::new("cmd-q", Quit, None),
    ]);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShimState {
    Installed,
    Missing,
    Conflict,
}

#[derive(Clone, Debug)]
enum PendingSwitch {
    NewProfile,
    SelectFolder(PathBuf),
}

#[derive(Clone, Default, PartialEq, Eq)]
struct Editor {
    folder: String,
    name: String,
    email: String,
    signing: String,
    gpg_sign: bool,
    signing_format: usize,
    ssh_key: String,
    https_helper: bool,
    mode: usize,
    user: String,
    app_id: String,
    installation: String,
    key: String,
    loaded_gpg_sign: Option<String>,
    loaded_gpg_format: Option<String>,
    loaded_ssh_cmd: Option<String>,
    loaded_cred_helper: Option<String>,
}

impl Editor {
    fn load(scope: &Scope, shim_path: &std::path::Path) -> anyhow::Result<Self> {
        let value = |key| crate::config::value(&scope.file, key);
        let raw_gpg_sign = value("commit.gpgSign")?;
        let raw_gpg_format = value("gpg.format")?;
        let raw_ssh_cmd = value("core.sshCommand")?;
        let raw_cred_helper = value("credential.helper")?;
        let expected_helper = format!("!{} auth git-credential", shim_path.display());

        Ok(Self {
            folder: scope.folder.display().to_string(),
            name: value("user.name")?,
            email: value("user.email")?,
            signing: value("user.signingKey")?,
            gpg_sign: raw_gpg_sign.eq_ignore_ascii_case("true"),
            signing_format: usize::from(raw_gpg_format.eq_ignore_ascii_case("ssh")),
            ssh_key: extract_ssh_key(&raw_ssh_cmd).unwrap_or_default(),
            https_helper: raw_cred_helper == expected_helper
                || raw_cred_helper.ends_with("gh auth git-credential"),
            mode: match value("github.authMode")?.as_str() {
                "personal" => 1,
                "app" => 2,
                _ => 0,
            },
            user: value("github.user")?,
            app_id: value("github.appId")?,
            installation: value("github.installationId")?,
            key: value("github.privateKey")?,
            loaded_gpg_sign: (!raw_gpg_sign.is_empty()).then_some(raw_gpg_sign),
            loaded_gpg_format: (!raw_gpg_format.is_empty()).then_some(raw_gpg_format),
            loaded_ssh_cmd: (!raw_ssh_cmd.is_empty()).then_some(raw_ssh_cmd),
            loaded_cred_helper: (!raw_cred_helper.is_empty()).then_some(raw_cred_helper),
        })
    }

    fn plan(&self, store: &Store, shim_path: &std::path::Path) -> anyhow::Result<Plan> {
        anyhow::ensure!(
            !self.folder.trim().is_empty(),
            "Choose a profile folder first"
        );
        let mut changes: Vec<_> = [
            ("user.name", &self.name),
            ("user.email", &self.email),
            ("user.signingKey", &self.signing),
            ("github.user", &self.user),
            ("github.appId", &self.app_id),
            ("github.installationId", &self.installation),
            ("github.privateKey", &self.key),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), (!value.is_empty()).then(|| value.clone())))
        .collect();

        changes.push((
            "github.authMode".into(),
            match self.mode {
                1 => Some("personal".into()),
                2 => Some("app".into()),
                _ => None,
            },
        ));

        if self.gpg_sign {
            changes.push(("commit.gpgSign".into(), Some("true".into())));
        } else if self.loaded_gpg_sign.is_some() {
            changes.push(("commit.gpgSign".into(), None));
        }

        if self.signing_format == 1 {
            changes.push(("gpg.format".into(), Some("ssh".into())));
        } else if self.loaded_gpg_format.is_some() {
            changes.push(("gpg.format".into(), None));
        }

        if !self.ssh_key.trim().is_empty() {
            changes.push((
                "core.sshCommand".into(),
                Some(ssh_command_for_key(&self.ssh_key)),
            ));
        } else if self
            .loaded_ssh_cmd
            .as_deref()
            .is_some_and(|cmd| extract_ssh_key(cmd).is_some())
        {
            changes.push(("core.sshCommand".into(), None));
        }

        let helper_cmd = format!("!{} auth git-credential", shim_path.display());
        if self.https_helper {
            changes.push(("credential.helper".into(), Some(helper_cmd)));
        } else if self
            .loaded_cred_helper
            .as_deref()
            .is_some_and(|h| h == helper_cmd || h.ends_with("gh auth git-credential"))
        {
            changes.push(("credential.helper".into(), None));
        }

        store.plan(&PathBuf::from(&self.folder), &changes)
    }
}

#[derive(Clone)]
struct Desktop {
    store: Store,
    scopes: Vec<Scope>,
    selected: Option<PathBuf>,
    editor: Editor,
    saved_editor: Editor,
    preview: Option<Plan>,
    restore_preview: Option<RestorePlan>,
    removal: Option<PathBuf>,
    replace_shim: bool,
    pending_switch: Option<PendingSwitch>,
    status: String,
    is_error: bool,
    filter: String,
}

impl Desktop {
    fn new(store: Store) -> Self {
        let mut app = Self {
            store,
            scopes: Vec::new(),
            selected: None,
            editor: Editor::default(),
            saved_editor: Editor::default(),
            preview: None,
            restore_preview: None,
            removal: None,
            replace_shim: false,
            pending_switch: None,
            status: "Select a profile or add a folder. Changes are saved only after preview."
                .into(),
            is_error: false,
            filter: String::new(),
        };
        if let Err(error) = app.reload() {
            app.status = format!("Cannot read profiles: {error:#}");
            app.is_error = true;
        }
        app
    }

    fn is_dirty(&self) -> bool {
        self.editor != self.saved_editor
    }

    fn has_modal(&self) -> bool {
        self.preview.is_some()
            || self.restore_preview.is_some()
            || self.removal.is_some()
            || self.replace_shim
            || self.pending_switch.is_some()
    }

    fn cancel_modal(&mut self) -> bool {
        if self.preview.is_some() {
            self.preview = None;
            true
        } else if self.restore_preview.is_some() {
            self.restore_preview = None;
            true
        } else if self.removal.is_some() {
            self.removal = None;
            true
        } else if self.replace_shim {
            self.replace_shim = false;
            true
        } else if self.pending_switch.is_some() {
            self.pending_switch = None;
            true
        } else {
            false
        }
    }

    fn reload(&mut self) -> anyhow::Result<()> {
        self.scopes = self.store.scopes()?;
        Ok(())
    }

    fn shim_dir(&self) -> PathBuf {
        self.store.home.join(".local/bin")
    }

    fn shim_path(&self) -> PathBuf {
        self.shim_dir().join("gh")
    }

    fn shim_state(&self) -> ShimState {
        let path = self.shim_path();
        if path.symlink_metadata().is_err() {
            return ShimState::Missing;
        }
        if let Ok(exe) = std::env::current_exe()
            && let (Ok(target), Ok(current)) = (std::fs::canonicalize(&path), exe.canonicalize())
            && target == current
        {
            return ShimState::Installed;
        }
        ShimState::Conflict
    }

    fn install_shim(&mut self, replace: bool) -> anyhow::Result<()> {
        let path = crate::auth::install_wrapper(&self.shim_dir(), replace)?;
        self.replace_shim = false;
        self.is_error = false;
        self.status = format!(
            "Installed gh shim at {}. Ensure ~/.local/bin precedes /usr/bin on PATH.",
            path.display()
        );
        Ok(())
    }

    fn verify_auth(&mut self) {
        match crate::auth::verify_profile_auth(
            self.editor.mode,
            &self.editor.user,
            &self.editor.app_id,
            &self.editor.installation,
            &self.editor.key,
            &self.store.home,
        ) {
            Ok(msg) => {
                self.is_error = false;
                self.status = msg;
            }
            Err(error) => {
                self.is_error = true;
                self.status = format!("Auth verification failed: {error:#}");
            }
        }
    }

    fn request_switch(&mut self, target: PendingSwitch) {
        if self.is_dirty() {
            self.pending_switch = Some(target);
        } else {
            self.apply_switch(target);
        }
    }

    fn apply_switch(&mut self, target: PendingSwitch) {
        self.pending_switch = None;
        match target {
            PendingSwitch::NewProfile => {
                self.selected = None;
                self.editor = Editor::default();
                self.saved_editor = Editor::default();
                self.is_error = false;
                self.status = "Creating a new folder profile.".into();
            }
            PendingSwitch::SelectFolder(folder) => {
                let result = self.select_folder(folder);
                self.report(result);
            }
        }
    }

    fn select_folder(&mut self, folder: PathBuf) -> anyhow::Result<()> {
        let existing = self.scopes.iter().find(|s| s.folder == folder);
        let scope = existing.cloned().unwrap_or_else(|| Scope {
            file: folder.join(".gitconfig"),
            folder: folder.clone(),
            condition: String::new(),
            name: String::new(),
            email: String::new(),
            auth: String::new(),
        });
        let editor = Editor::load(&scope, &self.shim_path())?;
        self.selected = existing.map(|s| s.folder.clone());
        self.saved_editor = editor.clone();
        self.editor = editor;
        self.preview = None;
        self.pending_switch = None;
        self.is_error = false;
        self.status = if self.selected.is_some() {
            format!("Loaded profile for {}", folder.display())
        } else {
            format!("Configuring new profile for {}", folder.display())
        };
        Ok(())
    }

    fn prepare(&mut self) -> anyhow::Result<()> {
        self.preview = None;
        self.preview = Some(self.editor.plan(&self.store, &self.shim_path())?);
        self.is_error = false;
        Ok(())
    }

    fn save(&mut self) -> anyhow::Result<()> {
        let plan = self
            .preview
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Preview changes first"))?;
        let backups = self.store.apply(plan)?;
        let folder = plan.folder.clone();
        self.preview = None;
        self.reload()?;
        self.select_folder(folder)?;
        self.is_error = false;
        self.status = format!(
            "Saved. {} backup(s) created (up to 5 retained per directory).",
            backups.len()
        );
        Ok(())
    }

    fn export_backup(&mut self, destination: &std::path::Path) -> anyhow::Result<()> {
        let bundle = self.store.export_backup(destination)?;
        self.is_error = false;
        self.status = format!(
            "Saved {} profile(s) to {}.",
            bundle.profiles.len(),
            destination.display()
        );
        Ok(())
    }

    fn prepare_restore(&mut self, source: &std::path::Path) -> anyhow::Result<()> {
        self.restore_preview = None;
        self.restore_preview = Some(self.store.plan_restore(source)?);
        self.is_error = false;
        Ok(())
    }

    fn apply_restore(&mut self) -> anyhow::Result<()> {
        let plan = self
            .restore_preview
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Preview restore first"))?;
        let count = plan.files.len();
        let first_folder = plan.files.first().map(|f| f.folder.clone());
        let backups = self.store.apply_restore(plan)?;
        self.restore_preview = None;
        self.reload()?;
        if let Some(folder) = self.selected.clone().or(first_folder) {
            let _ = self.select_folder(folder);
        }
        self.is_error = false;
        self.status = format!(
            "Restored {count} profile(s). {} safety backup(s) created.",
            backups.len()
        );
        Ok(())
    }

    fn remove(&mut self) -> anyhow::Result<()> {
        let folder = self
            .removal
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Select a profile first"))?;
        self.store.remove(folder)?;
        self.removal = None;
        self.selected = None;
        self.editor = Editor::default();
        self.saved_editor = Editor::default();
        self.reload()?;
        self.is_error = false;
        self.status = "Include removed. Configuration file retained.".into();
        Ok(())
    }

    fn report(&mut self, result: anyhow::Result<()>) {
        if let Err(error) = result {
            self.is_error = true;
            self.status = format!("{error:#}");
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct TextChanged;

struct TextInput {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    enabled: bool,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<gpui::ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    is_selecting: bool,
}

impl EventEmitter<TextChanged> for TextInput {}

impl TextInput {
    fn new(
        cx: &mut Context<Self>,
        initial: impl Into<SharedString>,
        placeholder: impl Into<SharedString>,
    ) -> Self {
        let content: SharedString = initial.into();
        let end = content.len();
        Self {
            focus_handle: cx.focus_handle().tab_stop(true),
            content,
            placeholder: placeholder.into(),
            enabled: true,
            selected_range: end..end,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            is_selecting: false,
        }
    }

    fn text(&self) -> &str {
        self.content.as_ref()
    }

    fn set_text(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        let next: SharedString = text.into();
        if self.content != next {
            let len = next.len();
            self.content = next;
            self.selected_range = len..len;
            self.selection_reversed = false;
            self.marked_range = None;
            cx.notify();
        }
    }

    fn set_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.enabled != enabled {
            self.enabled = enabled;
            cx.notify();
        }
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx);
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), cx);
        } else {
            self.move_to(self.selected_range.end, cx);
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx);
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if !self.enabled {
            return;
        }
        if self.selected_range.is_empty() {
            self.select_to(self.previous_boundary(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if !self.enabled {
            return;
        }
        if self.selected_range.is_empty() {
            self.select_to(self.next_boundary(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.enabled {
            return;
        }
        window.focus(&self.focus_handle);
        self.is_selecting = true;
        if event.modifiers.shift {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        } else {
            self.move_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting && self.enabled {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn show_character_palette(
        &mut self,
        _: &ShowCharacterPalette,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        if self.enabled {
            window.show_character_palette();
        }
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if !self.enabled {
            return;
        }
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let single_line = text.replace(['\r', '\n'], " ");
            self.replace_text_in_range(None, &single_line, window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.enabled {
            return;
        }
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
            self.replace_text_in_range(None, "", window, cx);
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let clamped = offset.min(self.content.len());
        self.selected_range = clamped..clamped;
        cx.notify();
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
        .min(self.content.len())
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }
        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        if position.x <= bounds.left() {
            return 0;
        }
        line.closest_index_for_x(position.x - bounds.left())
            .min(self.content.len())
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = offset.min(self.content.len());
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.notify();
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;
        for ch in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }
        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for ch in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }
        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(idx, _)| (idx > offset).then_some(idx))
            .unwrap_or(self.content.len())
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.enabled {
            return;
        }
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        let start = range.start.min(self.content.len());
        let end = range.end.min(self.content.len());
        self.content =
            (self.content[0..start].to_owned() + new_text + &self.content[end..]).into();
        self.selected_range = start + new_text.len()..start + new_text.len();
        self.marked_range.take();
        cx.emit(TextChanged);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.enabled {
            return;
        }
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        let start = range.start.min(self.content.len());
        let end = range.end.min(self.content.len());
        self.content =
            (self.content[0..start].to_owned() + new_text + &self.content[end..]).into();
        if !new_text.is_empty() {
            self.marked_range = Some(start..start + new_text.len());
        } else {
            self.marked_range = None;
        }
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .map(|new_range| new_range.start + start..new_range.end + end)
            .unwrap_or_else(|| start + new_text.len()..start + new_text.len());
        cx.emit(TextChanged);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let last_layout = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        Some(Bounds::from_corners(
            point(
                bounds.left() + last_layout.x_for_index(range.start),
                bounds.top(),
            ),
            point(
                bounds.left() + last_layout.x_for_index(range.end),
                bounds.bottom(),
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let line_point = self.last_bounds?.localize(&point)?;
        let last_layout = self.last_layout.as_ref()?;
        let utf8_index = last_layout.index_for_x(point.x - line_point.x)?;
        Some(self.offset_to_utf16(utf8_index))
    }
}

struct TextElement {
    input: Entity<TextInput>,
}

struct PrepaintState {
    line: Option<gpui::ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
    scroll_x: Pixels,
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let content = input.content.clone();
        let selected_range = input.selected_range.clone();
        let cursor = input.cursor_offset();
        let enabled = input.enabled;
        let style = window.text_style();

        let (display_text, text_color): (SharedString, Hsla) = if content.is_empty() {
            (input.placeholder.clone(), rgba(0x8e9aad99).into())
        } else if enabled {
            (content, rgb(TEXT_PRIMARY).into())
        } else {
            (content, rgb(TEXT_MUTED).into())
        };

        let run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = if let Some(marked_range) = input.marked_range.as_ref() {
            vec![
                TextRun {
                    len: marked_range.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked_range.end - marked_range.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: display_text.len().saturating_sub(marked_range.end),
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect()
        } else {
            vec![run]
        };

        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(display_text, font_size, &runs, None);

        let cursor_pos = line.x_for_index(cursor);
        let visible_w = (bounds.size.width - px(6.0)).max(px(1.0));
        let scroll_x = if cursor_pos > visible_w {
            cursor_pos - visible_w
        } else {
            px(0.0)
        };

        let (selection, cursor) = if selected_range.is_empty() {
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(bounds.left() + cursor_pos - scroll_x, bounds.top()),
                        size(px(1.5), bounds.bottom() - bounds.top()),
                    ),
                    rgb(ACCENT_TEAL),
                )),
            )
        } else {
            (
                Some(fill(
                    Bounds::from_corners(
                        point(
                            bounds.left() + line.x_for_index(selected_range.start) - scroll_x,
                            bounds.top(),
                        ),
                        point(
                            bounds.left() + line.x_for_index(selected_range.end) - scroll_x,
                            bounds.bottom(),
                        ),
                    ),
                    rgb(ACCENT_TEAL_DARK),
                )),
                None,
            )
        };

        PrepaintState {
            line: Some(line),
            cursor,
            selection,
            scroll_x,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (focus_handle, enabled) = {
            let input = self.input.read(cx);
            (input.focus_handle.clone(), input.enabled)
        };
        if enabled {
            window.handle_input(
                &focus_handle,
                ElementInputHandler::new(bounds, self.input.clone()),
                cx,
            );
        }
        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection);
        }
        let line = prepaint.line.take().unwrap();
        let paint_origin = point(bounds.origin.x - prepaint.scroll_x, bounds.origin.y);
        let _ = line.paint(paint_origin, window.line_height(), window, cx);

        if enabled
            && focus_handle.is_focused(window)
            && let Some(cursor) = prepaint.cursor.take()
        {
            window.paint_quad(cursor);
        }

        let scrolled_bounds = Bounds::new(paint_origin, bounds.size);
        self.input.update(cx, |input, _cx| {
            input.last_layout = Some(line);
            input.last_bounds = Some(scrolled_bounds);
        });
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let is_focused = self.enabled && self.focus_handle.is_focused(window);
        let border_color = if is_focused {
            rgb(BORDER_ACTIVE)
        } else {
            rgb(BORDER_SUBTLE)
        };

        div()
            .flex()
            .items_center()
            .w_full()
            .min_w_0()
            .h(px(32.0))
            .px(px(10.0))
            .rounded(px(8.0))
            .bg(rgb(BG_INPUT))
            .border_1()
            .border_color(border_color)
            .overflow_hidden()
            .key_context("TextInput")
            .track_focus(&self.focus_handle)
            .cursor(if self.enabled {
                CursorStyle::IBeam
            } else {
                CursorStyle::Arrow
            })
            .when(!self.enabled, |this| this.opacity(0.65))
            .text_size(px(13.0))
            .line_height(px(20.0))
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::show_character_palette))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .child(TextElement {
                input: cx.entity(),
            })
    }
}

struct DesktopView {
    app: Desktop,
    focus_handle: FocusHandle,
    filter_input: Entity<TextInput>,
    folder_input: Entity<TextInput>,
    name_input: Entity<TextInput>,
    email_input: Entity<TextInput>,
    signing_input: Entity<TextInput>,
    ssh_key_input: Entity<TextInput>,
    user_input: Entity<TextInput>,
    app_id_input: Entity<TextInput>,
    installation_input: Entity<TextInput>,
    key_input: Entity<TextInput>,
    _subscriptions: Vec<Subscription>,
}

impl Focusable for DesktopView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl DesktopView {
    fn new(app: Desktop, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let filter_input = cx.new(|cx| {
            TextInput::new(cx, app.filter.clone(), "Filter by name, path, or email…")
        });
        let folder_input = cx.new(|cx| {
            let mut input = TextInput::new(
                cx,
                app.editor.folder.clone(),
                "/home/user/work or ~/projects/client",
            );
            input.enabled = app.selected.is_none();
            input
        });
        let name_input =
            cx.new(|cx| TextInput::new(cx, app.editor.name.clone(), "e.g. Jane Doe"));
        let email_input =
            cx.new(|cx| TextInput::new(cx, app.editor.email.clone(), "e.g. jane@company.com"));
        let signing_input = cx.new(|cx| {
            TextInput::new(
                cx,
                app.editor.signing.clone(),
                "GPG key fingerprint or ~/.ssh/id_ed25519.pub",
            )
        });
        let ssh_key_input = cx.new(|cx| {
            TextInput::new(
                cx,
                app.editor.ssh_key.clone(),
                "~/.ssh/id_ed25519_work (leaves default SSH agent if empty)",
            )
        });
        let user_input =
            cx.new(|cx| TextInput::new(cx, app.editor.user.clone(), "e.g. octocat"));
        let app_id_input = cx.new(|cx| {
            TextInput::new(cx, app.editor.app_id.clone(), "Numeric GitHub App ID")
        });
        let installation_input = cx.new(|cx| {
            TextInput::new(
                cx,
                app.editor.installation.clone(),
                "Numeric Installation ID",
            )
        });
        let key_input = cx.new(|cx| {
            TextInput::new(cx, app.editor.key.clone(), "/path/to/private-key.pem")
        });

        let subscriptions = vec![
            cx.subscribe(&filter_input, |this, input, _: &TextChanged, cx| {
                this.app.filter = input.read(cx).text().to_owned();
                cx.notify();
            }),
            cx.subscribe(&folder_input, |this, input, _: &TextChanged, cx| {
                this.app.editor.folder = input.read(cx).text().to_owned();
                cx.notify();
            }),
            cx.subscribe(&name_input, |this, input, _: &TextChanged, cx| {
                this.app.editor.name = input.read(cx).text().to_owned();
                cx.notify();
            }),
            cx.subscribe(&email_input, |this, input, _: &TextChanged, cx| {
                this.app.editor.email = input.read(cx).text().to_owned();
                cx.notify();
            }),
            cx.subscribe(&signing_input, |this, input, _: &TextChanged, cx| {
                this.app.editor.signing = input.read(cx).text().to_owned();
                cx.notify();
            }),
            cx.subscribe(&ssh_key_input, |this, input, _: &TextChanged, cx| {
                this.app.editor.ssh_key = input.read(cx).text().to_owned();
                cx.notify();
            }),
            cx.subscribe(&user_input, |this, input, _: &TextChanged, cx| {
                this.app.editor.user = input.read(cx).text().to_owned();
                cx.notify();
            }),
            cx.subscribe(&app_id_input, |this, input, _: &TextChanged, cx| {
                this.app.editor.app_id = input.read(cx).text().to_owned();
                cx.notify();
            }),
            cx.subscribe(&installation_input, |this, input, _: &TextChanged, cx| {
                this.app.editor.installation = input.read(cx).text().to_owned();
                cx.notify();
            }),
            cx.subscribe(&key_input, |this, input, _: &TextChanged, cx| {
                this.app.editor.key = input.read(cx).text().to_owned();
                cx.notify();
            }),
        ];

        Self {
            app,
            focus_handle: cx.focus_handle(),
            filter_input,
            folder_input,
            name_input,
            email_input,
            signing_input,
            ssh_key_input,
            user_input,
            app_id_input,
            installation_input,
            key_input,
            _subscriptions: subscriptions,
        }
    }

    fn sync_inputs_from_app(&mut self, cx: &mut Context<Self>) {
        let modal = self.app.has_modal();
        let filter = self.app.filter.clone();
        let editor = self.app.editor.clone();
        let folder_enabled = self.app.selected.is_none() && !modal;

        self.filter_input.update(cx, |input, cx| {
            input.set_text(filter, cx);
            input.set_enabled(!modal, cx);
        });
        self.folder_input.update(cx, |input, cx| {
            input.set_text(editor.folder, cx);
            input.set_enabled(folder_enabled, cx);
        });
        self.name_input.update(cx, |input, cx| {
            input.set_text(editor.name, cx);
            input.set_enabled(!modal, cx);
        });
        self.email_input.update(cx, |input, cx| {
            input.set_text(editor.email, cx);
            input.set_enabled(!modal, cx);
        });
        self.signing_input.update(cx, |input, cx| {
            input.set_text(editor.signing, cx);
            input.set_enabled(!modal, cx);
        });
        self.ssh_key_input.update(cx, |input, cx| {
            input.set_text(editor.ssh_key, cx);
            input.set_enabled(!modal, cx);
        });
        self.user_input.update(cx, |input, cx| {
            input.set_text(editor.user, cx);
            input.set_enabled(!modal, cx);
        });
        self.app_id_input.update(cx, |input, cx| {
            input.set_text(editor.app_id, cx);
            input.set_enabled(!modal, cx);
        });
        self.installation_input.update(cx, |input, cx| {
            input.set_text(editor.installation, cx);
            input.set_enabled(!modal, cx);
        });
        self.key_input.update(cx, |input, cx| {
            input.set_text(editor.key, cx);
            input.set_enabled(!modal, cx);
        });
    }

    fn on_tab(&mut self, _: &Tab, window: &mut Window, _: &mut Context<Self>) {
        if !self.app.has_modal() {
            window.focus_next();
        }
    }

    fn on_tab_prev(&mut self, _: &TabPrev, window: &mut Window, _: &mut Context<Self>) {
        if !self.app.has_modal() {
            window.focus_prev();
        }
    }

    fn on_cancel_modal(&mut self, _: &CancelModal, _: &mut Window, cx: &mut Context<Self>) {
        if self.app.cancel_modal() {
            self.sync_inputs_from_app(cx);
            cx.notify();
        }
    }

    fn render_header(&mut self, cx: &mut Context<Self>) -> Div {
        let shim_state = self.app.shim_state();

        div()
            .w_full()
            .bg(rgb(BG_PANEL))
            .border_b_1()
            .border_color(rgb(BORDER_SUBTLE))
            .px(px(20.0))
            .py(px(12.0))
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap(px(10.0))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.0))
                    .child(draw_logo())
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(18.0))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(TEXT_PRIMARY))
                                    .child("Bipolar"),
                            )
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(rgb(TEXT_MUTED))
                                    .child("Directory-Scoped Git & GitHub Identity Manager"),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        primary_button("btn-new-profile", "+ New Profile").on_click(cx.listener(
                            |this, _, _, cx| {
                                this.app.request_switch(PendingSwitch::NewProfile);
                                this.sync_inputs_from_app(cx);
                                cx.notify();
                            },
                        )),
                    )
                    .child(
                        secondary_button("btn-reload", "⟳ Reload").on_click(cx.listener(
                            |this, _, _, cx| {
                                let result = this.app.reload();
                                this.app.report(result);
                                this.sync_inputs_from_app(cx);
                                cx.notify();
                            },
                        )),
                    )
                    .child(secondary_button("btn-backup", "Backup…").on_click(cx.listener(
                        |this, _, _, cx| {
                            if let Some(path) = rfd::FileDialog::new()
                                .set_title("Save Bipolar profiles backup")
                                .set_file_name("bipolar-backup.json")
                                .add_filter("JSON Backup", &["json"])
                                .save_file()
                            {
                                let result = this.app.export_backup(&path);
                                this.app.report(result);
                                cx.notify();
                            }
                        },
                    )))
                    .child(
                        secondary_button("btn-restore", "Restore…").on_click(cx.listener(
                            |this, _, _, cx| {
                                if let Some(path) = rfd::FileDialog::new()
                                    .set_title("Restore Bipolar profiles from backup")
                                    .add_filter("JSON Backup", &["json"])
                                    .pick_file()
                                {
                                    let result = this.app.prepare_restore(&path);
                                    this.app.report(result);
                                    this.sync_inputs_from_app(cx);
                                    cx.notify();
                                }
                            },
                        )),
                    )
                    .child(
                        secondary_button("btn-clean-backups", "Clean Backups").on_click(
                            cx.listener(|this, _, _, cx| {
                                match this.app.store.clean_backups() {
                                    Ok(removed) => {
                                        this.app.is_error = false;
                                        this.app.status =
                                            format!("Removed {removed} backup file(s).");
                                    }
                                    Err(error) => this.app.report(Err(error)),
                                }
                                cx.notify();
                            }),
                        ),
                    )
                    .child(self.render_shim_header_action(shim_state, cx)),
            )
    }

    fn render_shim_header_action(
        &mut self,
        shim_state: ShimState,
        cx: &mut Context<Self>,
    ) -> Div {
        match shim_state {
            ShimState::Installed => div()
                .flex()
                .flex_row()
                .items_center()
                .child(badge("✓ gh shim active", ACCENT_TEAL, ACCENT_TEAL_DARK)),
            ShimState::Missing => div().flex().flex_row().items_center().child(
                secondary_button("btn-install-shim-hdr", "⚡ Install gh Shim").on_click(
                    cx.listener(|this, _, _, cx| {
                        let result = this.app.install_shim(false);
                        this.app.report(result);
                        cx.notify();
                    }),
                ),
            ),
            ShimState::Conflict => div().flex().flex_row().items_center().child(
                secondary_button("btn-replace-shim-hdr", "⚡ Replace gh Shim…").on_click(
                    cx.listener(|this, _, _, cx| {
                        this.app.replace_shim = true;
                        this.sync_inputs_from_app(cx);
                        cx.notify();
                    }),
                ),
            ),
        }
    }

    fn render_status_bar(&self, compact_window: bool) -> Div {
        let dot_color = if self.app.is_error {
            DANGER_RED
        } else {
            ACCENT_TEAL
        };
        let text_color = if self.app.is_error {
            DANGER_RED
        } else {
            TEXT_MUTED
        };

        div()
            .w_full()
            .bg(rgb(BG_PANEL))
            .border_t_1()
            .border_color(rgb(BORDER_SUBTLE))
            .px(px(18.0))
            .py(px(8.0))
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap(px(8.0))
            .child(
                div()
                    .w(px(8.0))
                    .h(px(8.0))
                    .rounded(px(4.0))
                    .flex_shrink_0()
                    .bg(rgb(dot_color)),
            )
            .child(
                div()
                    .text_size(px(12.5))
                    .text_color(rgb(text_color))
                    .child(self.app.status.clone()),
            )
            .when(!compact_window, |this| {
                this.child(
                    div()
                        .text_size(px(11.5))
                        .text_color(rgb(TEXT_MUTED))
                        .child(format!("·  Global: {}", self.app.store.global.display())),
                )
            })
    }

    fn render_sidebar(
        &mut self,
        stacked: bool,
        compact_window: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let query = self.app.filter.trim().to_lowercase();
        let filtered_scopes: Vec<(usize, Scope)> = self
            .app
            .scopes
            .iter()
            .cloned()
            .enumerate()
            .filter(|(_, scope)| {
                if query.is_empty() {
                    return true;
                }
                let title = scope
                    .folder
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_lowercase();
                let folder_str = scope.folder.display().to_string().to_lowercase();
                title.contains(&query)
                    || folder_str.contains(&query)
                    || scope.email.to_lowercase().contains(&query)
                    || scope.name.to_lowercase().contains(&query)
            })
            .collect();

        let selected = self.app.selected.clone();
        let has_scopes = !self.app.scopes.is_empty();

        let container = div()
            .bg(rgb(BG_PANEL))
            .p(px(14.0))
            .flex()
            .flex_col()
            .gap(px(8.0));

        let container = if stacked {
            container
                .w_full()
                .max_h(px(230.0))
                .border_b_1()
                .border_color(rgb(BORDER_SUBTLE))
        } else {
            container
                .w(px(if compact_window { 235.0 } else { 290.0 }))
                .flex_shrink_0()
                .h_full()
                .border_r_1()
                .border_color(rgb(BORDER_SUBTLE))
        };

        container
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(TEXT_MUTED))
                            .child("PROFILES"),
                    )
                    .child(badge(
                        &self.app.scopes.len().to_string(),
                        ACCENT_TEAL,
                        ACCENT_TEAL_DARK,
                    )),
            )
            .child(self.filter_input.clone())
            .child(
                div()
                    .id("profiles-list-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .map(|list| {
                        if !has_scopes {
                            list.child(
                                div()
                                    .w_full()
                                    .p(px(14.0))
                                    .rounded(px(10.0))
                                    .bg(rgb(BG_CARD))
                                    .border_1()
                                    .border_color(rgb(BORDER_SUBTLE))
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.0))
                                    .child(
                                        div()
                                            .text_size(px(13.5))
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(rgb(TEXT_PRIMARY))
                                            .child("No folder profiles yet"),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.0))
                                            .text_color(rgb(TEXT_MUTED))
                                            .child(
                                                "Click '+ New Profile' to bind a directory to a Git identity and GitHub account.",
                                            ),
                                    ),
                            )
                        } else {
                            list.children(filtered_scopes.into_iter().map(|(ix, scope)| {
                                let is_selected = selected.as_ref() == Some(&scope.folder);
                                let folder = scope.folder.clone();
                                profile_card(ix, &scope, is_selected).on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.app.request_switch(PendingSwitch::SelectFolder(
                                            folder.clone(),
                                        ));
                                        this.sync_inputs_from_app(cx);
                                        cx.notify();
                                    },
                                ))
                            }))
                        }
                    }),
            )
    }

    fn render_editor(
        &mut self,
        compact_window: bool,
        two_col: bool,
        three_col: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let is_editing = self.app.selected.is_some();
        let dirty = self.app.is_dirty();
        let folder_str = self.app.editor.folder.trim().to_owned();
        let gpg_sign = self.app.editor.gpg_sign;
        let signing_format = self.app.editor.signing_format;
        let mode = self.app.editor.mode;
        let https_helper = self.app.editor.https_helper;
        let shim_dir = self.app.shim_dir();
        let shim_path = self.app.shim_path();
        let shim_state = self.app.shim_state();
        let path_ok = crate::auth::shim_precedes_usr_bin(&shim_dir);
        let h_pad = if compact_window { px(16.0) } else { px(24.0) };

        div()
            .id("editor-main-scroll")
            .flex_1()
            .min_w_0()
            .min_h_0()
            .h_full()
            .bg(rgb(BG_DARK))
            .overflow_y_scroll()
            .px(h_pad)
            .py(px(18.0))
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .text_size(px(20.0))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(TEXT_PRIMARY))
                                    .child(if is_editing {
                                        "Edit Folder Profile"
                                    } else {
                                        "New Folder Profile"
                                    }),
                            )
                            .child(if is_editing {
                                badge("ACTIVE INCLUDE", ACCENT_TEAL, ACCENT_TEAL_DARK)
                            } else {
                                badge("NEW SCOPE", ACCENT_PURPLE, ACCENT_PURPLE_DARK)
                            })
                            .when(dirty, |this| {
                                this.child(badge("MODIFIED", WARN_AMBER, WARN_BG))
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(12.5))
                            .text_color(rgb(TEXT_MUTED))
                            .child(
                                "Repositories inside this directory automatically inherit these settings. Unmanaged Git keys are preserved.",
                            ),
                    ),
            )
            .child(
                section_card(
                    "Target Directory",
                    "Git applies this profile via includeIf.gitdir for any repository underneath this path.",
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(180.0))
                                .child(self.folder_input.clone()),
                        )
                        .child(
                            secondary_button("btn-browse-folder", "Browse…").on_click(cx.listener(
                                |this, _, _, cx| {
                                    if let Some(path) = rfd::FileDialog::new()
                                        .set_title("Choose a profile folder")
                                        .pick_folder()
                                    {
                                        this.app
                                            .request_switch(PendingSwitch::SelectFolder(path));
                                        this.sync_inputs_from_app(cx);
                                        cx.notify();
                                    }
                                },
                            )),
                        ),
                )
                .when(!folder_str.is_empty(), |this| {
                    let target_cfg = PathBuf::from(&folder_str).join(".gitconfig");
                    this.child(
                        div()
                            .text_size(px(11.5))
                            .text_color(rgb(TEXT_MUTED))
                            .child(format!("Config file: {}", target_cfg.display())),
                    )
                }),
            )
            .child(
                section_card(
                    "Git Commit Identity & Signing",
                    "Author details, commit signing, and SSH identity for repositories inside this folder.",
                )
                .child(responsive_two_columns(
                    two_col,
                    field_with_hint("Author Name", self.name_input.clone()),
                    field_with_hint("Author Email", self.email_input.clone()),
                ))
                .child(responsive_two_columns(
                    two_col,
                    field_with_hint("Signing Key (optional)", self.signing_input.clone()),
                    self.render_signing_options(gpg_sign, signing_format, cx),
                ))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(
                            div()
                                .text_size(px(12.5))
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(TEXT_PRIMARY))
                                .child("Dedicated SSH Identity Key (optional, sets core.sshCommand)"),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .flex_wrap()
                                .items_center()
                                .gap(px(8.0))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(180.0))
                                        .child(self.ssh_key_input.clone()),
                                )
                                .child(
                                    secondary_button("btn-choose-ssh-key", "Choose SSH Key…")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            if let Some(path) = rfd::FileDialog::new()
                                                .set_title("Select SSH private key")
                                                .pick_file()
                                            {
                                                this.app.editor.ssh_key =
                                                    path.display().to_string();
                                                this.sync_inputs_from_app(cx);
                                                cx.notify();
                                            }
                                        })),
                                ),
                        ),
                ),
            )
            .child(
                section_card(
                    "GitHub Authentication & Credential Helper",
                    "Used by the Bipolar gh wrapper and Git HTTPS credential helper. Keys stay in their existing files.",
                )
                .child(
                    div()
                        .flex()
                        .map(|el| if three_col { el.flex_row() } else { el.flex_col() })
                        .gap(px(8.0))
                        .children(AUTH_MODES.iter().enumerate().map(|(index, (title, subtitle))| {
                            let active = mode == index;
                            mode_tile(index, title, subtitle, active).on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.app.editor.mode = index;
                                    cx.notify();
                                },
                            ))
                        })),
                )
                .child(self.render_auth_mode_details(mode, two_col, cx))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .items_center()
                        .gap(px(10.0))
                        .child(
                            checkbox_row(
                                "chk-https-helper",
                                https_helper,
                                "Use Bipolar gh shim as Git HTTPS credential helper",
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.app.editor.https_helper = !this.app.editor.https_helper;
                                cx.notify();
                            })),
                        )
                        .child(
                            secondary_button("btn-verify-auth", "✓ Verify Authentication")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.app.verify_auth();
                                    cx.notify();
                                })),
                        ),
                )
                .child(self.render_shim_box(shim_state, &shim_dir, &shim_path, path_ok, cx)),
            )
            .child(
                div()
                    .pt(px(4.0))
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        primary_button("btn-preview-save", "Preview & Save Changes…").on_click(
                            cx.listener(|this, _, _, cx| {
                                let result = this.app.prepare();
                                this.app.report(result);
                                this.sync_inputs_from_app(cx);
                                cx.notify();
                            }),
                        ),
                    )
                    .when(dirty, |this| {
                        this.child(
                            secondary_button("btn-revert-edits", "Revert Edits").on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.app.editor = this.app.saved_editor.clone();
                                    this.app.is_error = false;
                                    this.app.status = "Reverted unsaved changes.".into();
                                    this.sync_inputs_from_app(cx);
                                    cx.notify();
                                }),
                            ),
                        )
                    })
                    .when(is_editing, |this| {
                        this.child(
                            danger_button("btn-remove-include", "Remove Include…").on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.app.removal = this.app.selected.clone();
                                    this.sync_inputs_from_app(cx);
                                    cx.notify();
                                }),
                            ),
                        )
                    }),
            )
    }

    fn render_signing_options(
        &mut self,
        gpg_sign: bool,
        signing_format: usize,
        cx: &mut Context<Self>,
    ) -> Div {
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(
                div()
                    .text_size(px(12.5))
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(TEXT_PRIMARY))
                    .child("Commit Signing Options"),
            )
            .child(
                checkbox_row(
                    "chk-gpg-sign",
                    gpg_sign,
                    "Sign commits automatically (commit.gpgSign)",
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.app.editor.gpg_sign = !this.app.editor.gpg_sign;
                    cx.notify();
                })),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(rgb(TEXT_MUTED))
                            .child("Format:"),
                    )
                    .children(SIGNING_FORMATS.iter().enumerate().map(|(idx, label)| {
                        let active = signing_format == idx;
                        div()
                            .id(("signing-format-pill", idx))
                            .px(px(10.0))
                            .py(px(3.0))
                            .rounded(px(6.0))
                            .border_1()
                            .border_color(rgb(if active {
                                ACCENT_TEAL
                            } else {
                                BORDER_SUBTLE
                            }))
                            .bg(rgb(if active { ACCENT_TEAL_DARK } else { BG_INPUT }))
                            .text_size(px(12.0))
                            .font_weight(if active {
                                FontWeight::BOLD
                            } else {
                                FontWeight::NORMAL
                            })
                            .text_color(rgb(if active { ACCENT_TEAL } else { TEXT_PRIMARY }))
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(BG_CARD_HOVER)))
                            .child(*label)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.app.editor.signing_format = idx;
                                cx.notify();
                            }))
                    })),
            )
    }

    fn render_auth_mode_details(
        &mut self,
        mode: usize,
        two_col: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        match mode {
            1 => div().pt(px(4.0)).child(field_with_hint(
                "GitHub Username (already signed in with `gh auth login`)",
                self.user_input.clone(),
            )),
            2 => div()
                .pt(px(4.0))
                .flex()
                .flex_col()
                .gap(px(10.0))
                .child(responsive_two_columns(
                    two_col,
                    field_with_hint("App ID", self.app_id_input.clone()),
                    field_with_hint("Installation ID", self.installation_input.clone()),
                ))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(
                            div()
                                .text_size(px(12.5))
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(TEXT_PRIMARY))
                                .child("Private Key File (.pem, chmod 600)"),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .flex_wrap()
                                .items_center()
                                .gap(px(8.0))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(180.0))
                                        .child(self.key_input.clone()),
                                )
                                .child(
                                    secondary_button("btn-choose-pem-key", "Choose Key…").on_click(
                                        cx.listener(|this, _, _, cx| {
                                            if let Some(path) = rfd::FileDialog::new()
                                                .set_title("Select GitHub App private key")
                                                .pick_file()
                                            {
                                                this.app.editor.key = path.display().to_string();
                                                this.sync_inputs_from_app(cx);
                                                cx.notify();
                                            }
                                        }),
                                    ),
                                ),
                        ),
                ),
            _ => div(),
        }
    }

    fn render_shim_box(
        &mut self,
        shim_state: ShimState,
        shim_dir: &std::path::Path,
        shim_path: &std::path::Path,
        path_ok: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        div()
            .w_full()
            .px(px(12.0))
            .py(px(10.0))
            .rounded(px(8.0))
            .bg(rgb(BG_INPUT))
            .border_1()
            .border_color(rgb(BORDER_SUBTLE))
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .text_size(px(13.0))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(TEXT_PRIMARY))
                            .child("Native Rust gh Shim"),
                    )
                    .child(match shim_state {
                        ShimState::Installed => badge("INSTALLED", ACCENT_TEAL, ACCENT_TEAL_DARK),
                        ShimState::Missing => {
                            badge("NOT INSTALLED", ACCENT_PURPLE, ACCENT_PURPLE_DARK)
                        }
                        ShimState::Conflict => badge("EXISTING FILE", DANGER_RED, DANGER_BG),
                    })
                    .when(!path_ok, |this| {
                        this.child(badge("PATH ORDER WARNING", WARN_AMBER, WARN_BG))
                    })
                    .child(match shim_state {
                        ShimState::Installed => {
                            secondary_button("btn-reinstall-shim", "Reinstall Shim").on_click(
                                cx.listener(|this, _, _, cx| {
                                    let result = this.app.install_shim(true);
                                    this.app.report(result);
                                    cx.notify();
                                }),
                            )
                        }
                        ShimState::Missing => primary_button("btn-install-shim", "Install gh Shim")
                            .on_click(cx.listener(|this, _, _, cx| {
                                let result = this.app.install_shim(false);
                                this.app.report(result);
                                cx.notify();
                            })),
                        ShimState::Conflict => secondary_button(
                            "btn-replace-existing-gh",
                            "Replace Existing gh…",
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.app.replace_shim = true;
                            this.sync_inputs_from_app(cx);
                            cx.notify();
                        })),
                    }),
            )
            .child(
                div()
                    .text_size(px(11.5))
                    .text_color(rgb(TEXT_MUTED))
                    .child(format!(
                        "{}  ·  Helper: !{} auth git-credential",
                        shim_path.display(),
                        shim_path.display()
                    )),
            )
            .when(!path_ok, |this| {
                this.child(
                    div()
                        .text_size(px(11.5))
                        .text_color(rgb(WARN_AMBER))
                        .child(format!(
                            "Note: {} is not before /usr/bin on PATH. Add it to PATH so `gh` invokes the shim.",
                            shim_dir.display()
                        )),
                )
            })
    }

    fn render_modal(&mut self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        if let Some(plan) = self.app.preview.clone() {
            return Some(self.render_preview_modal(&plan, cx));
        }
        if let Some(plan) = self.app.restore_preview.clone() {
            return Some(self.render_restore_modal(&plan, cx));
        }
        if let Some(folder) = self.app.removal.clone() {
            return Some(self.render_remove_modal(&folder, cx));
        }
        if self.app.replace_shim {
            let shim_path = self.app.shim_path();
            return Some(self.render_replace_shim_modal(&shim_path, cx));
        }
        if let Some(target) = self.app.pending_switch.clone() {
            return Some(self.render_discard_modal(target, cx));
        }
        None
    }

    fn render_preview_modal(&mut self, plan: &Plan, cx: &mut Context<Self>) -> Stateful<Div> {
        let text = plan.preview();
        let is_error = self.app.is_error;
        let status = self.app.status.clone();

        modal_backdrop("modal-preview-backdrop").child(
            modal_card("modal-preview-card", px(760.0))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(
                            div()
                                .text_size(px(18.0))
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(TEXT_PRIMARY))
                                .child("Review Configuration Changes"),
                        )
                        .child(
                            div()
                                .text_size(px(12.5))
                                .text_color(rgb(TEXT_MUTED))
                                .child(
                                    "Verify the exact before/after configuration files before applying. Backups are created automatically.",
                                ),
                        ),
                )
                .child(
                    div()
                        .id("preview-diff-scroll")
                        .w_full()
                        .max_h(px(440.0))
                        .p(px(12.0))
                        .rounded(px(8.0))
                        .bg(rgb(BG_INPUT))
                        .border_1()
                        .border_color(rgb(BORDER_SUBTLE))
                        .overflow_y_scroll()
                        .child(render_diff(&text)),
                )
                .when(is_error, |this| {
                    this.child(
                        div()
                            .text_size(px(12.5))
                            .text_color(rgb(DANGER_RED))
                            .child(status),
                    )
                })
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            secondary_button("btn-modal-cancel-preview", "Cancel").on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.app.preview = None;
                                    this.sync_inputs_from_app(cx);
                                    cx.notify();
                                }),
                            ),
                        )
                        .child(
                            primary_button("btn-modal-save-config", "Save Configuration").on_click(
                                cx.listener(|this, _, _, cx| {
                                    let result = this.app.save();
                                    this.app.report(result);
                                    this.sync_inputs_from_app(cx);
                                    cx.notify();
                                }),
                            ),
                        ),
                ),
        )
    }

    fn render_restore_modal(
        &mut self,
        plan: &RestorePlan,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let text = plan.preview();
        let count = plan.files.len();
        let is_error = self.app.is_error;
        let status = self.app.status.clone();

        modal_backdrop("modal-restore-backdrop").child(
            modal_card("modal-restore-card", px(760.0))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(
                            div()
                                .text_size(px(18.0))
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(TEXT_PRIMARY))
                                .child(format!("Restore {count} Profile(s) from Backup")),
                        )
                        .child(
                            div()
                                .text_size(px(12.5))
                                .text_color(rgb(TEXT_MUTED))
                                .child(
                                    "Review the profile files and global includes that will be created or updated. Safety backups are created automatically.",
                                ),
                        ),
                )
                .child(
                    div()
                        .id("restore-diff-scroll")
                        .w_full()
                        .max_h(px(440.0))
                        .p(px(12.0))
                        .rounded(px(8.0))
                        .bg(rgb(BG_INPUT))
                        .border_1()
                        .border_color(rgb(BORDER_SUBTLE))
                        .overflow_y_scroll()
                        .child(render_diff(&text)),
                )
                .when(is_error, |this| {
                    this.child(
                        div()
                            .text_size(px(12.5))
                            .text_color(rgb(DANGER_RED))
                            .child(status),
                    )
                })
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            secondary_button("btn-modal-restore-cancel", "Cancel").on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.app.restore_preview = None;
                                    this.sync_inputs_from_app(cx);
                                    cx.notify();
                                }),
                            ),
                        )
                        .child(
                            primary_button("btn-modal-restore-confirm", "Restore Profiles")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let result = this.app.apply_restore();
                                    this.app.report(result);
                                    this.sync_inputs_from_app(cx);
                                    cx.notify();
                                })),
                        ),
                ),
        )
    }

    fn render_remove_modal(
        &mut self,
        folder: &std::path::Path,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let is_error = self.app.is_error;
        let status = self.app.status.clone();

        modal_backdrop("modal-remove-backdrop").child(
            modal_card("modal-remove-card", px(440.0))
                .child(
                    div()
                        .text_size(px(18.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(TEXT_PRIMARY))
                        .child("Remove this folder include?"),
                )
                .child(
                    div()
                        .w_full()
                        .p(px(10.0))
                        .rounded(px(8.0))
                        .bg(rgb(BG_INPUT))
                        .border_1()
                        .border_color(rgb(BORDER_SUBTLE))
                        .text_size(px(13.0))
                        .text_color(rgb(ACCENT_TEAL))
                        .child(folder.display().to_string()),
                )
                .child(
                    div()
                        .text_size(px(12.5))
                        .text_color(rgb(TEXT_MUTED))
                        .child(
                            "Only the includeIf entry in your global .gitconfig will be removed. The folder's .gitconfig file will be kept untouched.",
                        ),
                )
                .when(is_error, |this| {
                    this.child(
                        div()
                            .text_size(px(12.5))
                            .text_color(rgb(DANGER_RED))
                            .child(status),
                    )
                })
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            secondary_button("btn-modal-remove-cancel", "Cancel").on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.app.removal = None;
                                    this.sync_inputs_from_app(cx);
                                    cx.notify();
                                }),
                            ),
                        )
                        .child(
                            danger_button("btn-modal-remove-confirm", "Remove Include").on_click(
                                cx.listener(|this, _, _, cx| {
                                    let result = this.app.remove();
                                    this.app.report(result);
                                    this.sync_inputs_from_app(cx);
                                    cx.notify();
                                }),
                            ),
                        ),
                ),
        )
    }

    fn render_replace_shim_modal(
        &mut self,
        shim_path: &std::path::Path,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        modal_backdrop("modal-replace-shim-backdrop").child(
            modal_card("modal-replace-shim-card", px(460.0))
                .child(
                    div()
                        .text_size(px(18.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(TEXT_PRIMARY))
                        .child("Replace existing gh executable?"),
                )
                .child(
                    div()
                        .w_full()
                        .p(px(10.0))
                        .rounded(px(8.0))
                        .bg(rgb(BG_INPUT))
                        .border_1()
                        .border_color(rgb(BORDER_SUBTLE))
                        .text_size(px(13.0))
                        .text_color(rgb(ACCENT_TEAL))
                        .child(shim_path.display().to_string()),
                )
                .child(
                    div()
                        .text_size(px(12.5))
                        .text_color(rgb(TEXT_MUTED))
                        .child(
                            "The existing file will be backed up alongside it before creating the symlink to Bipolar.",
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            secondary_button("btn-modal-replace-shim-cancel", "Cancel").on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.app.replace_shim = false;
                                    this.sync_inputs_from_app(cx);
                                    cx.notify();
                                }),
                            ),
                        )
                        .child(
                            primary_button("btn-modal-replace-shim-confirm", "Backup & Replace")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let result = this.app.install_shim(true);
                                    this.app.report(result);
                                    this.sync_inputs_from_app(cx);
                                    cx.notify();
                                })),
                        ),
                ),
        )
    }

    fn render_discard_modal(
        &mut self,
        target: PendingSwitch,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        modal_backdrop("modal-discard-backdrop").child(
            modal_card("modal-discard-card", px(420.0))
                .child(
                    div()
                        .text_size(px(18.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(TEXT_PRIMARY))
                        .child("Discard unsaved changes?"),
                )
                .child(
                    div()
                        .text_size(px(12.5))
                        .text_color(rgb(TEXT_MUTED))
                        .child(
                            "You have modified the current profile without saving. Switching profiles will discard your edits.",
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            secondary_button("btn-modal-discard-cancel", "Keep Editing").on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.app.pending_switch = None;
                                    this.sync_inputs_from_app(cx);
                                    cx.notify();
                                }),
                            ),
                        )
                        .child(
                            danger_button("btn-modal-discard-confirm", "Discard & Switch")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.app.apply_switch(target.clone());
                                    this.sync_inputs_from_app(cx);
                                    cx.notify();
                                })),
                        ),
                ),
        )
    }
}

impl Render for DesktopView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_inputs_from_app(cx);

        let viewport = window.viewport_size();
        let viewport_w: f32 = viewport.width.into();
        let compact_window = viewport_w < COMPACT_WINDOW_BREAKPOINT;
        let stacked_sidebar = viewport_w < STACKED_SIDEBAR_BREAKPOINT;
        let sidebar_w = if stacked_sidebar {
            0.0
        } else if compact_window {
            235.0
        } else {
            290.0
        };
        let editor_w = (viewport_w - sidebar_w - 40.0).max(240.0);
        let two_col = editor_w >= TWO_COLUMN_BREAKPOINT;
        let three_col = editor_w >= THREE_COLUMN_BREAKPOINT;

        let header = self.render_header(cx);
        let sidebar = self.render_sidebar(stacked_sidebar, compact_window, cx);
        let editor = self.render_editor(compact_window, two_col, three_col, cx);
        let status_bar = self.render_status_bar(compact_window);
        let modal = self.render_modal(cx);

        div()
            .id("bipolar-root")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_tab))
            .on_action(cx.listener(Self::on_tab_prev))
            .on_action(cx.listener(Self::on_cancel_modal))
            .size_full()
            .relative()
            .bg(rgb(BG_DARK))
            .text_color(rgb(TEXT_PRIMARY))
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(header)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    .map(|body| {
                        if stacked_sidebar {
                            body.flex_col()
                        } else {
                            body.flex_row()
                        }
                    })
                    .overflow_hidden()
                    .child(sidebar)
                    .child(editor),
            )
            .child(status_bar)
            .children(modal)
    }
}

fn draw_logo() -> Div {
    div()
        .w(px(32.0))
        .h(px(32.0))
        .rounded(px(8.0))
        .bg(rgb(0x202630))
        .border_1()
        .border_color(rgb(BORDER_SUBTLE))
        .flex_shrink_0()
        .child(
            canvas(
                move |_, _, _| {},
                move |bounds, _, window, _| {
                    let origin = bounds.origin;
                    let left_top = point(origin.x + px(10.0), origin.y + px(9.0));
                    let left_bottom = point(origin.x + px(10.0), origin.y + px(23.0));
                    let right_top = point(origin.x + px(22.0), origin.y + px(9.0));
                    let mid = point(origin.x + px(10.0), origin.y + px(17.0));

                    let mut lines = PathBuilder::stroke(px(2.2));
                    lines.move_to(left_top);
                    lines.line_to(left_bottom);
                    lines.move_to(right_top);
                    lines.line_to(mid);
                    if let Ok(path) = lines.build() {
                        window.paint_path(path, rgb(TEXT_PRIMARY));
                    }

                    let r = px(3.2);
                    for (center, color) in [
                        (left_top, ACCENT_TEAL),
                        (left_bottom, ACCENT_TEAL),
                        (right_top, ACCENT_PURPLE),
                    ] {
                        window.paint_quad(
                            fill(
                                Bounds::new(
                                    point(center.x - r, center.y - r),
                                    size(r * 2.0, r * 2.0),
                                ),
                                rgb(color),
                            )
                            .corner_radii(r),
                        );
                    }
                },
            )
            .size_full(),
        )
}

fn badge(text: &str, fg: u32, bg_color: u32) -> Div {
    div()
        .px(px(8.0))
        .py(px(3.0))
        .rounded(px(6.0))
        .bg(rgb(bg_color))
        .border_1()
        .border_color(rgb(fg))
        .text_size(px(10.5))
        .font_weight(FontWeight::BOLD)
        .text_color(rgb(fg))
        .flex_shrink_0()
        .child(text.to_owned())
}

fn primary_button(id: impl Into<ElementId>, label: &str) -> Stateful<Div> {
    div()
        .id(id)
        .px(px(12.0))
        .py(px(7.0))
        .rounded(px(8.0))
        .bg(rgb(ACCENT_TEAL))
        .border_1()
        .border_color(rgb(ACCENT_TEAL))
        .text_size(px(13.5))
        .font_weight(FontWeight::BOLD)
        .text_color(rgb(0x0c1214))
        .cursor_pointer()
        .flex_shrink_0()
        .hover(|style| style.opacity(0.9))
        .active(|style| style.opacity(0.8))
        .child(label.to_owned())
}

fn secondary_button(id: impl Into<ElementId>, label: &str) -> Stateful<Div> {
    div()
        .id(id)
        .px(px(12.0))
        .py(px(7.0))
        .rounded(px(8.0))
        .bg(rgb(BG_CARD))
        .border_1()
        .border_color(rgb(BORDER_SUBTLE))
        .text_size(px(13.5))
        .text_color(rgb(TEXT_PRIMARY))
        .cursor_pointer()
        .flex_shrink_0()
        .hover(|style| {
            style
                .bg(rgb(BG_CARD_HOVER))
                .border_color(rgb(ACCENT_TEAL))
        })
        .active(|style| style.opacity(0.85))
        .child(label.to_owned())
}

fn danger_button(id: impl Into<ElementId>, label: &str) -> Stateful<Div> {
    div()
        .id(id)
        .px(px(12.0))
        .py(px(7.0))
        .rounded(px(8.0))
        .bg(rgb(DANGER_BG))
        .border_1()
        .border_color(rgb(DANGER_RED))
        .text_size(px(13.5))
        .font_weight(FontWeight::BOLD)
        .text_color(rgb(DANGER_RED))
        .cursor_pointer()
        .flex_shrink_0()
        .hover(|style| style.bg(rgb(0x59222a)))
        .active(|style| style.opacity(0.85))
        .child(label.to_owned())
}

fn checkbox_row(id: impl Into<ElementId>, checked: bool, label: &str) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .cursor_pointer()
        .child(
            div()
                .w(px(16.0))
                .h(px(16.0))
                .rounded(px(4.0))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(rgb(if checked { ACCENT_TEAL_DARK } else { BG_INPUT }))
                .border_1()
                .border_color(rgb(if checked { ACCENT_TEAL } else { BORDER_SUBTLE }))
                .text_size(px(11.0))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(ACCENT_TEAL))
                .child(if checked { "✓" } else { "" }),
        )
        .child(
            div()
                .text_size(px(13.0))
                .text_color(rgb(TEXT_PRIMARY))
                .child(label.to_owned()),
        )
}

fn profile_card(ix: usize, scope: &Scope, selected: bool) -> Stateful<Div> {
    let title = scope
        .folder
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let folder_str = scope.folder.display().to_string();
    let display_title = if title.is_empty() {
        folder_str.clone()
    } else {
        title.into_owned()
    };

    let (label, fg, bg) = match scope.auth.as_str() {
        "personal" => ("GH USER", ACCENT_TEAL, ACCENT_TEAL_DARK),
        "app" => ("GH APP", ACCENT_PURPLE, ACCENT_PURPLE_DARK),
        _ => ("GIT ONLY", TEXT_MUTED, BG_INPUT),
    };

    let identity = match (scope.name.is_empty(), scope.email.is_empty()) {
        (false, false) => format!("{} <{}>", scope.name, scope.email),
        (false, true) => scope.name.clone(),
        (true, false) => scope.email.clone(),
        (true, true) => "No commit author configured".into(),
    };

    div()
        .id(("profile-card", ix))
        .w_full()
        .px(px(12.0))
        .py(px(10.0))
        .rounded(px(10.0))
        .bg(rgb(if selected { BG_CARD_ACTIVE } else { BG_CARD }))
        .border_1()
        .border_color(rgb(if selected {
            BORDER_ACTIVE
        } else {
            BORDER_SUBTLE
        }))
        .cursor_pointer()
        .hover(|style| {
            style
                .bg(rgb(BG_CARD_HOVER))
                .border_color(rgb(ACCENT_TEAL))
        })
        .flex()
        .flex_col()
        .gap(px(3.0))
        .overflow_hidden()
        .child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_center()
                .justify_between()
                .gap(px(6.0))
                .child(
                    div()
                        .text_size(px(14.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(if selected { ACCENT_TEAL } else { TEXT_PRIMARY }))
                        .overflow_hidden()
                        .child(display_title),
                )
                .child(badge(label, fg, bg)),
        )
        .child(
            div()
                .text_size(px(11.0))
                .text_color(rgb(TEXT_MUTED))
                .overflow_hidden()
                .child(folder_str),
        )
        .child(
            div()
                .text_size(px(11.5))
                .text_color(rgb(TEXT_PRIMARY))
                .overflow_hidden()
                .child(identity),
        )
}

fn section_card(title: &str, subtitle: &str) -> Div {
    div()
        .w_full()
        .p(px(16.0))
        .rounded(px(12.0))
        .bg(rgb(BG_CARD))
        .border_1()
        .border_color(rgb(BORDER_SUBTLE))
        .flex()
        .flex_col()
        .gap(px(10.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(
                    div()
                        .text_size(px(15.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(TEXT_PRIMARY))
                        .child(title.to_owned()),
                )
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(rgb(TEXT_MUTED))
                        .child(subtitle.to_owned()),
                ),
        )
}

fn responsive_two_columns(two_col: bool, left: impl IntoElement, right: impl IntoElement) -> Div {
    if two_col {
        div()
            .w_full()
            .flex()
            .flex_row()
            .items_start()
            .gap(px(12.0))
            .child(div().flex_1().min_w_0().child(left))
            .child(div().flex_1().min_w_0().child(right))
    } else {
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(left)
            .child(right)
    }
}

fn field_with_hint(label: &str, input: Entity<TextInput>) -> Div {
    div()
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(
            div()
                .text_size(px(12.5))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(TEXT_PRIMARY))
                .child(label.to_owned()),
        )
        .child(input)
}

fn mode_tile(ix: usize, title: &str, subtitle: &str, active: bool) -> Stateful<Div> {
    div()
        .id(("auth-mode-tile", ix))
        .flex_1()
        .min_w_0()
        .px(px(12.0))
        .py(px(9.0))
        .rounded(px(9.0))
        .bg(rgb(if active { BG_CARD_ACTIVE } else { BG_INPUT }))
        .border_1()
        .border_color(rgb(if active { ACCENT_TEAL } else { BORDER_SUBTLE }))
        .cursor_pointer()
        .hover(|style| {
            style
                .bg(rgb(BG_CARD_HOVER))
                .border_color(rgb(ACCENT_TEAL))
        })
        .flex()
        .flex_col()
        .gap(px(3.0))
        .child(
            div()
                .text_size(px(13.5))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(if active { ACCENT_TEAL } else { TEXT_PRIMARY }))
                .child(title.to_owned()),
        )
        .child(
            div()
                .text_size(px(11.5))
                .text_color(rgb(TEXT_MUTED))
                .child(subtitle.to_owned()),
        )
}

fn modal_backdrop(id: impl Into<ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .occlude()
        .absolute()
        .inset_0()
        .bg(rgba(0x05070acc))
        .flex()
        .items_center()
        .justify_center()
        .p(px(16.0))
}

fn modal_card(id: impl Into<ElementId>, max_width: Pixels) -> Stateful<Div> {
    div()
        .id(id)
        .occlude()
        .w_full()
        .max_w(max_width)
        .max_h(relative(0.92))
        .p(px(18.0))
        .rounded(px(12.0))
        .bg(rgb(BG_PANEL))
        .border_1()
        .border_color(rgb(BORDER_SUBTLE))
        .flex()
        .flex_col()
        .gap(px(12.0))
        .overflow_hidden()
}

fn render_diff(text: &str) -> Div {
    let lines: Vec<Div> = text
        .lines()
        .map(|line| {
            let color = if line.starts_with("---") {
                DANGER_RED
            } else if line.starts_with("+++") {
                ACCENT_TEAL
            } else if line == "CURRENT:" || line == "PROPOSED:" {
                ACCENT_PURPLE
            } else {
                TEXT_PRIMARY
            };
            div()
                .text_size(px(12.5))
                .text_color(rgb(color))
                .child(line.to_owned())
        })
        .collect();

    div().flex().flex_col().gap(px(3.0)).children(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    fn fixture() -> (tempfile::TempDir, Desktop, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let folder = temp.path().join("personal");
        std::fs::create_dir(&folder).unwrap();
        let store = Store {
            global: temp.path().join("global.gitconfig"),
            home: temp.path().to_path_buf(),
        };
        let mut app = Desktop::new(store);
        app.select_folder(folder.clone()).unwrap();
        app.editor.name = "Personal Developer".into();
        app.editor.email = "personal@example.com".into();
        (temp, app, folder)
    }

    fn render_at(app: &mut Desktop, width: f32, height: f32) {
        let mut cx = TestAppContext::single();
        let snapshot = app.clone();
        let (_view, vcx) = cx.add_window_view(|window, cx| DesktopView::new(snapshot, window, cx));
        vcx.simulate_resize(size(px(width), px(height)));
        vcx.run_until_parked();
    }

    fn render(app: &mut Desktop) {
        render_at(app, 1180.0, 860.0);
        render_at(app, 420.0, 700.0);
    }

    #[test]
    fn desktop_preview_save_and_remove() {
        let (_temp, mut app, folder) = fixture();
        render(&mut app);
        assert!(app.save().is_err());
        app.prepare().unwrap();
        render(&mut app);
        assert!(!app.store.global.exists());
        assert!(!folder.join(".gitconfig").exists());
        app.save().unwrap();
        assert_eq!(app.scopes.len(), 1);
        assert_eq!(
            crate::config::value(&folder.join(".gitconfig"), "user.name").unwrap(),
            "Personal Developer"
        );
        assert_eq!(app.selected, Some(folder.clone()));
        app.removal = app.selected.clone();
        render(&mut app);
        app.remove().unwrap();
        assert!(app.store.scopes().unwrap().is_empty());
        assert!(folder.join(".gitconfig").exists());
    }

    #[test]
    fn cancelled_and_stale_previews_do_not_write() {
        let (_temp, mut app, folder) = fixture();
        app.prepare().unwrap();
        app.preview = None;
        assert!(app.save().is_err());
        assert!(!app.store.global.exists());
        app.prepare().unwrap();
        std::fs::write(folder.join(".gitconfig"), "[user]\nname = External\n").unwrap();
        assert!(app.save().is_err());
        assert!(!app.store.global.exists());
        assert_eq!(
            crate::config::value(&folder.join(".gitconfig"), "user.name").unwrap(),
            "External"
        );
    }

    #[test]
    fn existing_profile_loads_auth_and_preserves_extra_settings() {
        let (_temp, mut app, folder) = fixture();
        let file = folder.join(".gitconfig");
        std::fs::write(&file, "# Keep me\n[user]\nname = Existing\n[github]\nauthMode = app\nappId = 123\ninstallationId = 456\nprivateKey = /tmp/example.pem\n[commit]\ngpgSign = true\n").unwrap();
        app.select_folder(folder).unwrap();
        assert_eq!(app.editor.mode, 2);
        assert_eq!(app.editor.app_id, "123");
        assert_eq!(app.editor.installation, "456");
        assert_eq!(app.editor.key, "/tmp/example.pem");
        assert!(app.editor.gpg_sign);
        assert!(!app.is_dirty());
        render(&mut app);
        app.editor.mode = 1;
        app.editor.user = "personal-user".into();
        app.editor.ssh_key = "~/.ssh/id_ed25519_work".into();
        app.editor.signing_format = 1;
        app.editor.https_helper = true;
        assert!(app.is_dirty());
        render(&mut app);
        app.prepare().unwrap();
        app.save().unwrap();
        assert!(!app.is_dirty());
        assert_eq!(
            crate::config::value(&file, "github.authMode").unwrap(),
            "personal"
        );
        assert_eq!(
            crate::config::value(&file, "github.user").unwrap(),
            "personal-user"
        );
        assert_eq!(
            crate::config::value(&file, "commit.gpgSign").unwrap(),
            "true"
        );
        assert_eq!(crate::config::value(&file, "gpg.format").unwrap(), "ssh");
        assert_eq!(
            crate::config::value(&file, "core.sshCommand").unwrap(),
            "ssh -i '~/.ssh/id_ed25519_work' -o IdentitiesOnly=yes"
        );
        assert!(
            crate::config::value(&file, "credential.helper")
                .unwrap()
                .ends_with("gh auth git-credential")
        );
        assert!(std::fs::read_to_string(file).unwrap().contains("# Keep me"));
    }

    #[test]
    fn desktop_installs_and_detects_gh_shim() {
        let (_temp, mut app, _folder) = fixture();
        assert_eq!(app.shim_state(), ShimState::Missing);
        app.install_shim(false).unwrap();
        assert_eq!(app.shim_state(), ShimState::Installed);
        assert!(app.shim_path().is_symlink());
        std::fs::remove_file(app.shim_path()).unwrap();
        std::fs::write(app.shim_path(), "existing-gh").unwrap();
        assert_eq!(app.shim_state(), ShimState::Conflict);
        app.replace_shim = true;
        render(&mut app);
        app.install_shim(true).unwrap();
        assert_eq!(app.shim_state(), ShimState::Installed);
    }

    #[test]
    fn dirty_state_prompts_before_switching_profile() {
        let (_temp, mut app, _folder) = fixture();
        assert!(app.is_dirty());
        app.request_switch(PendingSwitch::NewProfile);
        assert!(app.pending_switch.is_some());
        render(&mut app);
        app.apply_switch(PendingSwitch::NewProfile);
        assert!(!app.is_dirty());
        assert!(app.editor.folder.is_empty());
    }

    #[test]
    fn desktop_backup_and_restore_across_machines() {
        let (temp_a, mut app_a, _folder_a) = fixture();
        app_a.prepare().unwrap();
        app_a.save().unwrap();
        let backup_file = temp_a.path().join("bipolar-backup.json");
        app_a.export_backup(&backup_file).unwrap();
        assert!(backup_file.is_file());

        let temp_b = tempfile::tempdir().unwrap();
        let store_b = Store {
            global: temp_b.path().join(".gitconfig"),
            home: temp_b.path().to_path_buf(),
        };
        let mut app_b = Desktop::new(store_b);
        app_b.prepare_restore(&backup_file).unwrap();
        assert!(app_b.restore_preview.is_some());
        render(&mut app_b);
        app_b.apply_restore().unwrap();
        assert_eq!(app_b.scopes.len(), 1);
        assert_eq!(app_b.scopes[0].folder, temp_b.path().join("personal"));
        assert_eq!(app_b.scopes[0].name, "Personal Developer");
        assert_eq!(app_b.scopes[0].email, "personal@example.com");
    }
}
