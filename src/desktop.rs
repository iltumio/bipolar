use crate::config::{Plan, RestorePlan, Scope, Store, extract_ssh_key, ssh_command_for_key};
use eframe::egui::{
    self, Align, Color32, CornerRadius, FontFamily, FontId, Frame, Id, Key, Layout, Margin,
    RichText, ScrollArea, Sense, Stroke, TextEdit, TextStyle, Vec2,
};
use std::path::PathBuf;

pub fn run(store: Store) -> anyhow::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 860.0])
            .with_min_inner_size([400.0, 420.0])
            .with_app_id("dev.iltumio.Bipolar"),
        ..Default::default()
    };
    eframe::run_native(
        "Bipolar",
        options,
        Box::new(move |cc| {
            apply_theme(&cc.egui_ctx);
            Ok(Box::new(Desktop::new(store)))
        }),
    )
    .map_err(|error| anyhow::anyhow!("Cannot run desktop app: {error}"))
}

const BG_DARK: Color32 = Color32::from_rgb(15, 18, 24);
const BG_PANEL: Color32 = Color32::from_rgb(20, 24, 32);
const BG_CARD: Color32 = Color32::from_rgb(26, 31, 41);
const BG_CARD_HOVER: Color32 = Color32::from_rgb(33, 39, 52);
const BG_INPUT: Color32 = Color32::from_rgb(17, 21, 28);
const BORDER_SUBTLE: Color32 = Color32::from_rgb(42, 50, 66);
const BORDER_ACTIVE: Color32 = Color32::from_rgb(100, 216, 200);
const TEXT_PRIMARY: Color32 = Color32::from_rgb(232, 237, 242);
const TEXT_MUTED: Color32 = Color32::from_rgb(142, 154, 173);
const ACCENT_TEAL: Color32 = Color32::from_rgb(100, 216, 200);
const ACCENT_TEAL_DARK: Color32 = Color32::from_rgb(22, 68, 64);
const ACCENT_PURPLE: Color32 = Color32::from_rgb(179, 156, 251);
const ACCENT_PURPLE_DARK: Color32 = Color32::from_rgb(48, 38, 82);
const WARN_AMBER: Color32 = Color32::from_rgb(245, 185, 95);
const WARN_BG: Color32 = Color32::from_rgb(68, 48, 20);
const DANGER_RED: Color32 = Color32::from_rgb(242, 110, 110);
const DANGER_BG: Color32 = Color32::from_rgb(66, 26, 32);

const COMPACT_WINDOW_BREAKPOINT: f32 = 820.0;
const TWO_COLUMN_BREAKPOINT: f32 = 540.0;
const THREE_COLUMN_BREAKPOINT: f32 = 620.0;

const AUTH_MODES: [(&str, &str); 3] = [
    ("Inherit", "Use global Git / gh credentials"),
    ("Personal Login", "Switch gh CLI user automatically"),
    ("GitHub App", "Mint short-lived installation tokens"),
];

const SIGNING_FORMATS: [&str; 2] = ["GPG (openpgp)", "SSH (ssh)"];

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

fn apply_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    let mut visuals = egui::Visuals::dark();

    visuals.panel_fill = BG_DARK;
    visuals.window_fill = BG_PANEL;
    visuals.extreme_bg_color = BG_INPUT;
    visuals.faint_bg_color = BG_CARD;
    visuals.override_text_color = Some(TEXT_PRIMARY);
    visuals.window_corner_radius = CornerRadius::same(12);
    visuals.window_stroke = Stroke::new(1.0, BORDER_SUBTLE);

    visuals.widgets.noninteractive.bg_fill = BG_CARD;
    visuals.widgets.noninteractive.weak_bg_fill = BG_CARD;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER_SUBTLE);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(8);

    visuals.widgets.inactive.bg_fill = BG_CARD;
    visuals.widgets.inactive.weak_bg_fill = BG_CARD;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER_SUBTLE);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    visuals.widgets.inactive.corner_radius = CornerRadius::same(8);

    visuals.widgets.hovered.bg_fill = BG_CARD_HOVER;
    visuals.widgets.hovered.weak_bg_fill = BG_CARD_HOVER;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT_TEAL);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(8);

    visuals.widgets.active.bg_fill = ACCENT_TEAL_DARK;
    visuals.widgets.active.weak_bg_fill = ACCENT_TEAL_DARK;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT_TEAL);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    visuals.widgets.active.corner_radius = CornerRadius::same(8);

    visuals.selection.bg_fill = ACCENT_TEAL_DARK;
    visuals.selection.stroke = Stroke::new(1.0, ACCENT_TEAL);

    style.visuals = visuals;
    style.spacing.item_spacing = Vec2::new(8.0, 8.0);
    style.spacing.button_padding = Vec2::new(12.0, 7.0);
    style.spacing.window_margin = Margin::same(16);

    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(19.0, FontFamily::Proportional),
    );
    style
        .text_styles
        .insert(TextStyle::Body, FontId::new(14.0, FontFamily::Proportional));
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(13.5, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Small,
        FontId::new(12.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Monospace,
        FontId::new(12.5, FontFamily::Monospace),
    );

    ctx.set_style(style);
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
    styled: bool,
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
            styled: false,
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
            self.status = format!("{error:#}");
            self.is_error = true;
        }
    }

    fn show(&mut self, ctx: &egui::Context) {
        if !self.styled {
            apply_theme(ctx);
            self.styled = true;
        }
        let modal = self.preview.is_some()
            || self.restore_preview.is_some()
            || self.removal.is_some()
            || self.replace_shim
            || self.pending_switch.is_some();

        let window_width = ctx.content_rect().width();
        let compact_window = window_width < COMPACT_WINDOW_BREAKPOINT;

        egui::TopBottomPanel::top("header")
            .frame(
                Frame::new()
                    .fill(BG_PANEL)
                    .inner_margin(Margin::symmetric(if compact_window { 12 } else { 20 }, 12))
                    .stroke(Stroke::new(1.0, BORDER_SUBTLE)),
            )
            .show(ctx, |ui| {
                ui.add_enabled_ui(!modal, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        draw_logo(ui);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            ui.label(
                                RichText::new("Bipolar")
                                    .size(17.0)
                                    .strong()
                                    .color(TEXT_PRIMARY),
                            );
                            if !compact_window {
                                ui.label(
                                    RichText::new("Folder-scoped Git identities & GitHub auth")
                                        .size(11.5)
                                        .color(TEXT_MUTED),
                                );
                            }
                        });

                        ui.add_space(6.0);

                        if primary_button(ui, "+ New Profile").clicked() {
                            self.request_switch(PendingSwitch::NewProfile);
                        }
                        if secondary_button(ui, "⟳ Reload").clicked() {
                            let result = self.reload().and_then(|()| {
                                if let Some(folder) = self.selected.clone() {
                                    self.select_folder(folder)?;
                                }
                                self.is_error = false;
                                self.status = "Configurations reloaded.".into();
                                Ok(())
                            });
                            self.report(result);
                        }
                        if secondary_button(ui, "Backup…").clicked()
                            && let Some(path) = rfd::FileDialog::new()
                                .set_title("Save Bipolar Backup")
                                .set_file_name("bipolar-backup.json")
                                .add_filter("JSON Backup", &["json"])
                                .save_file()
                        {
                            let result = self.export_backup(&path);
                            self.report(result);
                        }
                        if secondary_button(ui, "Restore…").clicked()
                            && let Some(path) = rfd::FileDialog::new()
                                .set_title("Restore Bipolar Backup")
                                .add_filter("JSON Backup", &["json"])
                                .pick_file()
                        {
                            let result = self.prepare_restore(&path);
                            self.report(result);
                        }
                        if secondary_button(ui, "Clean Backups").clicked() {
                            match self.store.clean_backups() {
                                Ok(count) => {
                                    self.is_error = false;
                                    self.status = format!("Removed {count} backup file(s).");
                                }
                                Err(err) => self.report(Err(err)),
                            }
                        }
                        self.shim_header_action(ui);
                    });
                });
            });

        egui::TopBottomPanel::bottom("status")
            .frame(
                Frame::new()
                    .fill(BG_PANEL)
                    .inner_margin(Margin::symmetric(if compact_window { 12 } else { 20 }, 8))
                    .stroke(Stroke::new(1.0, BORDER_SUBTLE)),
            )
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let (dot_color, text_color) = if self.is_error {
                        (DANGER_RED, DANGER_RED)
                    } else {
                        (ACCENT_TEAL, TEXT_MUTED)
                    };
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
                    ui.painter().circle_filled(rect.center(), 4.0, dot_color);
                    ui.label(RichText::new(&self.status).size(12.0).color(text_color));
                    if !compact_window {
                        ui.label(
                            RichText::new(format!("· Global: {}", self.store.global.display()))
                                .font(FontId::new(11.0, FontFamily::Monospace))
                                .color(TEXT_MUTED),
                        );
                    }
                });
            });

        egui::SidePanel::left("profiles")
            .default_width(275.0)
            .min_width(180.0)
            .max_width((window_width * 0.45).max(200.0))
            .frame(
                Frame::new()
                    .fill(BG_PANEL)
                    .inner_margin(Margin::same(12))
                    .stroke(Stroke::new(1.0, BORDER_SUBTLE)),
            )
            .show(ctx, |ui| {
                ui.add_enabled_ui(!modal, |ui| {
                    let sb_w = ui.available_width();
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("FOLDER PROFILES")
                                .size(11.5)
                                .strong()
                                .color(TEXT_MUTED),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            badge(
                                ui,
                                &self.scopes.len().to_string(),
                                ACCENT_TEAL,
                                ACCENT_TEAL_DARK,
                            );
                        });
                    });
                    ui.add_space(4.0);

                    full_width_text_edit(
                        ui,
                        sb_w,
                        &mut self.filter,
                        "Filter profiles…",
                        false,
                        true,
                    );
                    ui.add_space(6.0);

                    let mut chosen = None;
                    let query = self.filter.trim().to_lowercase();
                    ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let card_w = ui.available_width();
                            if self.scopes.is_empty() {
                                let inner_w = (card_w - 30.0).max(0.0);
                                Frame::new()
                                    .fill(BG_CARD)
                                    .corner_radius(CornerRadius::same(10))
                                    .inner_margin(Margin::same(14))
                                    .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                                    .show(ui, |ui| {
                                        ui.set_min_width(inner_w);
                                        ui.set_max_width(inner_w);
                                        ui.label(
                                            RichText::new("No folder profiles yet")
                                                .strong()
                                                .color(TEXT_PRIMARY),
                                        );
                                        ui.add_space(4.0);
                                        ui.label(
                                            RichText::new(
                                                "Click '+ New Profile' to bind a directory to a Git identity and GitHub account.",
                                            )
                                            .size(12.0)
                                            .color(TEXT_MUTED),
                                        );
                                    });
                            }
                            for scope in &self.scopes {
                                let title = scope
                                    .folder
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy();
                                let folder_str = scope.folder.display().to_string();
                                if !query.is_empty()
                                    && !title.to_lowercase().contains(&query)
                                    && !folder_str.to_lowercase().contains(&query)
                                    && !scope.email.to_lowercase().contains(&query)
                                    && !scope.name.to_lowercase().contains(&query)
                                {
                                    continue;
                                }
                                let is_selected = self.selected.as_ref() == Some(&scope.folder);
                                if profile_card(ui, scope, &title, &folder_str, is_selected, card_w)
                                {
                                    chosen = Some(scope.folder.clone());
                                }
                                ui.add_space(6.0);
                            }
                        });
                    if let Some(folder) = chosen {
                        self.request_switch(PendingSwitch::SelectFolder(folder));
                    }
                });
            });

        let h_pad: i8 = if compact_window { 16 } else { 24 };
        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(BG_DARK)
                    .inner_margin(Margin::symmetric(h_pad, 18)),
            )
            .show(ctx, |ui| {
                ui.add_enabled_ui(!modal, |ui| {
                    ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let content_w = ui.available_width();
                            ui.set_min_width(content_w);
                            ui.set_max_width(content_w);
                            self.editor_view(ui, content_w);
                        });
                });
            });

        self.dialogs(ctx);
    }

    fn shim_header_action(&mut self, ui: &mut egui::Ui) {
        match self.shim_state() {
            ShimState::Installed => {
                badge(ui, "✓ gh shim active", ACCENT_TEAL, ACCENT_TEAL_DARK);
            }
            ShimState::Missing => {
                if secondary_button(ui, "⚡ Install gh Shim").clicked() {
                    let result = self.install_shim(false);
                    self.report(result);
                }
            }
            ShimState::Conflict => {
                if secondary_button(ui, "⚡ Replace gh Shim…").clicked() {
                    self.replace_shim = true;
                }
            }
        }
    }

    fn editor_view(&mut self, ui: &mut egui::Ui, content_w: f32) {
        let dirty = self.is_dirty();
        let two_col = content_w >= TWO_COLUMN_BREAKPOINT;
        let three_col = content_w >= THREE_COLUMN_BREAKPOINT;

        ui.horizontal_wrapped(|ui| {
            let heading = if self.selected.is_some() {
                "Edit Folder Profile"
            } else {
                "New Folder Profile"
            };
            ui.label(
                RichText::new(heading)
                    .size(20.0)
                    .strong()
                    .color(TEXT_PRIMARY),
            );
            if self.selected.is_some() {
                badge(ui, "ACTIVE INCLUDE", ACCENT_TEAL, ACCENT_TEAL_DARK);
            } else {
                badge(ui, "NEW SCOPE", ACCENT_PURPLE, ACCENT_PURPLE_DARK);
            }
            if dirty {
                badge(ui, "MODIFIED", WARN_AMBER, WARN_BG);
            }
        });
        ui.label(
            RichText::new(
                "Repositories inside this directory automatically inherit these settings. Unmanaged Git keys are preserved.",
            )
            .size(12.5)
            .color(TEXT_MUTED),
        );

        ui.add_space(12.0);

        section_card(
            ui,
            content_w,
            "Target Directory",
            "Git applies this profile via includeIf.gitdir for any repository underneath this path.",
            |ui, inner_w| {
                file_picker_row_enabled(
                    ui,
                    inner_w,
                    self.selected.is_none(),
                    &mut self.editor.folder,
                    "/home/user/work or ~/projects/client",
                    "Browse…",
                    92.0,
                )
                .then(|| {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_title("Choose a profile folder")
                        .pick_folder()
                    {
                        self.request_switch(PendingSwitch::SelectFolder(path));
                    }
                });
                if !self.editor.folder.trim().is_empty() {
                    let target_cfg = PathBuf::from(self.editor.folder.trim()).join(".gitconfig");
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new(format!("Config file: {}", target_cfg.display()))
                            .font(FontId::new(11.5, FontFamily::Monospace))
                            .color(TEXT_MUTED),
                    );
                }
            },
        );

        ui.add_space(12.0);

        section_card(
            ui,
            content_w,
            "Git Commit Identity & Signing",
            "Author details, commit signing, and SSH identity for repositories inside this folder.",
            |ui, inner_w| {
                if two_col {
                    two_column_row(
                        ui,
                        inner_w,
                        |col, w| {
                            field_with_hint(
                                col,
                                w,
                                "Author Name",
                                "e.g. Jane Doe",
                                &mut self.editor.name,
                                false,
                            );
                        },
                        |col, w| {
                            field_with_hint(
                                col,
                                w,
                                "Author Email",
                                "e.g. jane@company.com",
                                &mut self.editor.email,
                                false,
                            );
                        },
                    );
                } else {
                    field_with_hint(
                        ui,
                        inner_w,
                        "Author Name",
                        "e.g. Jane Doe",
                        &mut self.editor.name,
                        false,
                    );
                    field_with_hint(
                        ui,
                        inner_w,
                        "Author Email",
                        "e.g. jane@company.com",
                        &mut self.editor.email,
                        false,
                    );
                }

                ui.add_space(2.0);

                if two_col {
                    let signing = &mut self.editor.signing;
                    let gpg_sign = &mut self.editor.gpg_sign;
                    let signing_format = &mut self.editor.signing_format;
                    two_column_row(
                        ui,
                        inner_w,
                        |col, w| {
                            field_with_hint(
                                col,
                                w,
                                "Signing Key (optional)",
                                "GPG key fingerprint or ~/.ssh/id_ed25519.pub",
                                signing,
                                true,
                            );
                        },
                        |col, _w| {
                            signing_options_ui(col, gpg_sign, signing_format);
                        },
                    );
                } else {
                    field_with_hint(
                        ui,
                        inner_w,
                        "Signing Key (optional)",
                        "GPG key fingerprint or ~/.ssh/id_ed25519.pub",
                        &mut self.editor.signing,
                        true,
                    );
                    signing_options_ui(
                        ui,
                        &mut self.editor.gpg_sign,
                        &mut self.editor.signing_format,
                    );
                }

                ui.add_space(4.0);
                ui.label(
                    RichText::new("Dedicated SSH Identity Key (optional, sets core.sshCommand)")
                        .size(12.5)
                        .strong()
                        .color(TEXT_PRIMARY),
                );
                file_picker_row(
                    ui,
                    inner_w,
                    &mut self.editor.ssh_key,
                    "~/.ssh/id_ed25519_work (leaves default SSH agent if empty)",
                    "Choose SSH Key…",
                    136.0,
                    "Select SSH private key",
                );
            },
        );

        ui.add_space(12.0);

        let shim_dir = self.shim_dir();
        let shim_path = self.shim_path();
        let shim_state = self.shim_state();
        let path_ok = crate::auth::shim_precedes_usr_bin(&shim_dir);
        let mut install_clicked = false;
        let mut replace_clicked = false;
        let mut verify_clicked = false;

        section_card(
            ui,
            content_w,
            "GitHub Authentication & Credential Helper",
            "Used by the Bipolar gh wrapper and Git HTTPS credential helper. Keys stay in their existing files.",
            |ui, inner_w| {
                if three_col {
                    let gap = ui.spacing().item_spacing.x;
                    let col_w = ((inner_w - gap * 2.0) / 3.0).max(80.0);
                    ui.horizontal(|ui| {
                        for (index, (title, subtitle)) in AUTH_MODES.iter().enumerate() {
                            let active = self.editor.mode == index;
                            if mode_tile(ui, col_w, title, subtitle, active) {
                                self.editor.mode = index;
                            }
                        }
                    });
                } else {
                    for (index, (title, subtitle)) in AUTH_MODES.iter().enumerate() {
                        let active = self.editor.mode == index;
                        if mode_tile(ui, inner_w, title, subtitle, active) {
                            self.editor.mode = index;
                        }
                        if index + 1 < AUTH_MODES.len() {
                            ui.add_space(6.0);
                        }
                    }
                }

                match self.editor.mode {
                    1 => {
                        ui.add_space(8.0);
                        field_with_hint(
                            ui,
                            inner_w,
                            "GitHub Username (already signed in with `gh auth login`)",
                            "e.g. octocat",
                            &mut self.editor.user,
                            false,
                        );
                    }
                    2 => {
                        ui.add_space(8.0);
                        if two_col {
                            two_column_row(
                                ui,
                                inner_w,
                                |col, w| {
                                    field_with_hint(
                                        col,
                                        w,
                                        "App ID",
                                        "Numeric GitHub App ID",
                                        &mut self.editor.app_id,
                                        true,
                                    );
                                },
                                |col, w| {
                                    field_with_hint(
                                        col,
                                        w,
                                        "Installation ID",
                                        "Numeric Installation ID",
                                        &mut self.editor.installation,
                                        true,
                                    );
                                },
                            );
                        } else {
                            field_with_hint(
                                ui,
                                inner_w,
                                "App ID",
                                "Numeric GitHub App ID",
                                &mut self.editor.app_id,
                                true,
                            );
                            field_with_hint(
                                ui,
                                inner_w,
                                "Installation ID",
                                "Numeric Installation ID",
                                &mut self.editor.installation,
                                true,
                            );
                        }
                        ui.add_space(2.0);
                        ui.label(
                            RichText::new("Private Key File (.pem, chmod 600)")
                                .size(12.5)
                                .strong()
                                .color(TEXT_PRIMARY),
                        );
                        file_picker_row(
                            ui,
                            inner_w,
                            &mut self.editor.key,
                            "/path/to/private-key.pem",
                            "Choose Key…",
                            112.0,
                            "Select GitHub App private key",
                        );
                    }
                    _ => {}
                }

                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(
                        &mut self.editor.https_helper,
                        "Use Bipolar gh shim as Git HTTPS credential helper",
                    );
                    if secondary_button(ui, "✓ Verify Authentication").clicked() {
                        verify_clicked = true;
                    }
                });

                ui.add_space(6.0);
                let shim_box_inner = (inner_w - 26.0).max(0.0);
                Frame::new()
                    .fill(BG_INPUT)
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::symmetric(12, 10))
                    .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                    .show(ui, |ui| {
                        ui.set_min_width(shim_box_inner);
                        ui.set_max_width(shim_box_inner);
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new("Native Rust gh Shim")
                                    .size(13.0)
                                    .strong()
                                    .color(TEXT_PRIMARY),
                            );
                            match shim_state {
                                ShimState::Installed => {
                                    badge(ui, "INSTALLED", ACCENT_TEAL, ACCENT_TEAL_DARK);
                                }
                                ShimState::Missing => {
                                    badge(ui, "NOT INSTALLED", ACCENT_PURPLE, ACCENT_PURPLE_DARK);
                                }
                                ShimState::Conflict => {
                                    badge(ui, "EXISTING FILE", DANGER_RED, DANGER_BG);
                                }
                            }
                            if !path_ok {
                                badge(ui, "PATH ORDER WARNING", WARN_AMBER, WARN_BG);
                            }
                            match shim_state {
                                ShimState::Installed => {
                                    if secondary_button(ui, "Reinstall Shim").clicked() {
                                        install_clicked = true;
                                    }
                                }
                                ShimState::Missing => {
                                    if primary_button(ui, "Install gh Shim").clicked() {
                                        install_clicked = true;
                                    }
                                }
                                ShimState::Conflict => {
                                    if secondary_button(ui, "Replace Existing gh…").clicked() {
                                        replace_clicked = true;
                                    }
                                }
                            }
                        });
                        ui.label(
                            RichText::new(format!(
                                "{}  ·  Helper: !{} auth git-credential",
                                shim_path.display(),
                                shim_path.display()
                            ))
                            .font(FontId::new(11.5, FontFamily::Monospace))
                            .color(TEXT_MUTED),
                        );
                        if !path_ok {
                            ui.label(
                                RichText::new(format!(
                                    "Note: {} is not before /usr/bin on PATH. Add it to PATH so `gh` invokes the shim.",
                                    shim_dir.display()
                                ))
                                .size(11.5)
                                .color(WARN_AMBER),
                            );
                        }
                    });
            },
        );

        if verify_clicked {
            self.verify_auth();
        }
        if install_clicked {
            let replace = shim_state == ShimState::Installed;
            let result = self.install_shim(replace);
            self.report(result);
        }
        if replace_clicked {
            self.replace_shim = true;
        }

        ui.add_space(16.0);

        ui.horizontal_wrapped(|ui| {
            if primary_button(ui, "Preview & Save Changes…").clicked() {
                let result = self.prepare();
                self.report(result);
            }
            if dirty && secondary_button(ui, "Revert Edits").clicked() {
                self.editor = self.saved_editor.clone();
                self.is_error = false;
                self.status = "Reverted unsaved changes.".into();
            }
            if self.selected.is_some() && danger_button(ui, "Remove Include…").clicked() {
                self.removal = self.selected.clone();
            }
        });
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        let max_modal_w = (ctx.content_rect().width() - 32.0).max(260.0);
        if let Some(plan) = &self.preview {
            let text = plan.preview();
            let mut save = false;
            let mut cancel = false;
            if ctx.input(|i| i.key_pressed(Key::Escape)) {
                cancel = true;
            }
            egui::Modal::new(Id::new("preview")).show(ctx, |ui| {
                ui.set_width(max_modal_w.min(760.0));
                ui.label(
                    RichText::new("Review Configuration Changes")
                        .size(18.0)
                        .strong()
                        .color(TEXT_PRIMARY),
                );
                ui.label(
                    RichText::new(
                        "Verify the exact before/after configuration files before applying. Backups are created automatically.",
                    )
                    .size(12.5)
                    .color(TEXT_MUTED),
                );
                ui.add_space(10.0);

                Frame::new()
                    .fill(BG_INPUT)
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::same(12))
                    .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                    .show(ui, |ui| {
                        ScrollArea::both()
                            .max_height(ctx.content_rect().height() * 0.55)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                render_diff(ui, &text);
                            });
                    });

                if self.is_error {
                    ui.add_space(6.0);
                    ui.label(RichText::new(&self.status).color(DANGER_RED).size(12.5));
                }

                ui.add_space(12.0);
                ui.horizontal_wrapped(|ui| {
                    if secondary_button(ui, "Cancel").clicked() {
                        cancel = true;
                    }
                    if primary_button(ui, "Save Configuration").clicked() {
                        save = true;
                    }
                });
            });
            if cancel {
                self.preview = None;
            } else if save {
                let result = self.save();
                self.report(result);
            }
        }
        if let Some(plan) = &self.restore_preview {
            let text = plan.preview();
            let count = plan.files.len();
            let mut confirm = false;
            let mut cancel = false;
            if ctx.input(|i| i.key_pressed(Key::Escape)) {
                cancel = true;
            }
            egui::Modal::new(Id::new("restore-preview")).show(ctx, |ui| {
                ui.set_width(max_modal_w.min(760.0));
                ui.label(
                    RichText::new(format!("Restore {count} Profile(s) from Backup"))
                        .size(18.0)
                        .strong()
                        .color(TEXT_PRIMARY),
                );
                ui.label(
                    RichText::new(
                        "Review the profile files and global includes that will be created or updated. Safety backups are created automatically.",
                    )
                    .size(12.5)
                    .color(TEXT_MUTED),
                );
                ui.add_space(10.0);

                Frame::new()
                    .fill(BG_INPUT)
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::same(12))
                    .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                    .show(ui, |ui| {
                        ScrollArea::both()
                            .max_height(ctx.content_rect().height() * 0.55)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                render_diff(ui, &text);
                            });
                    });

                if self.is_error {
                    ui.add_space(6.0);
                    ui.label(RichText::new(&self.status).color(DANGER_RED).size(12.5));
                }

                ui.add_space(12.0);
                ui.horizontal_wrapped(|ui| {
                    if secondary_button(ui, "Cancel").clicked() {
                        cancel = true;
                    }
                    if primary_button(ui, "Restore Profiles").clicked() {
                        confirm = true;
                    }
                });
            });
            if cancel {
                self.restore_preview = None;
            } else if confirm {
                let result = self.apply_restore();
                self.report(result);
            }
        }
        if let Some(folder) = &self.removal {
            let mut remove = false;
            let mut cancel = false;
            if ctx.input(|i| i.key_pressed(Key::Escape)) {
                cancel = true;
            }
            egui::Modal::new(Id::new("remove")).show(ctx, |ui| {
                ui.set_width(max_modal_w.min(440.0));
                ui.label(
                    RichText::new("Remove this folder include?")
                        .size(18.0)
                        .strong()
                        .color(TEXT_PRIMARY),
                );
                ui.add_space(6.0);
                Frame::new()
                    .fill(BG_INPUT)
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::same(10))
                    .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(folder.display().to_string())
                                .font(FontId::new(13.0, FontFamily::Monospace))
                                .color(ACCENT_TEAL),
                        );
                    });
                ui.add_space(6.0);
                ui.label(
                    RichText::new(
                        "Only the includeIf entry in your global .gitconfig will be removed. The folder's .gitconfig file will be kept untouched.",
                    )
                    .size(12.5)
                    .color(TEXT_MUTED),
                );
                if self.is_error {
                    ui.add_space(6.0);
                    ui.label(RichText::new(&self.status).color(DANGER_RED).size(12.5));
                }
                ui.add_space(12.0);
                ui.horizontal_wrapped(|ui| {
                    if secondary_button(ui, "Cancel").clicked() {
                        cancel = true;
                    }
                    if danger_button(ui, "Remove Include").clicked() {
                        remove = true;
                    }
                });
            });
            if cancel {
                self.removal = None;
            } else if remove {
                let result = self.remove();
                self.report(result);
            }
        }
        if self.replace_shim {
            let shim_path = self.shim_path();
            let mut confirm = false;
            let mut cancel = false;
            if ctx.input(|i| i.key_pressed(Key::Escape)) {
                cancel = true;
            }
            egui::Modal::new(Id::new("replace-shim")).show(ctx, |ui| {
                ui.set_width(max_modal_w.min(460.0));
                ui.label(
                    RichText::new("Replace existing gh executable?")
                        .size(18.0)
                        .strong()
                        .color(TEXT_PRIMARY),
                );
                ui.add_space(6.0);
                Frame::new()
                    .fill(BG_INPUT)
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::same(10))
                    .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(shim_path.display().to_string())
                                .font(FontId::new(13.0, FontFamily::Monospace))
                                .color(ACCENT_TEAL),
                        );
                    });
                ui.add_space(6.0);
                ui.label(
                    RichText::new(
                        "The existing file will be backed up alongside it before creating the symlink to Bipolar.",
                    )
                    .size(12.5)
                    .color(TEXT_MUTED),
                );
                ui.add_space(12.0);
                ui.horizontal_wrapped(|ui| {
                    if secondary_button(ui, "Cancel").clicked() {
                        cancel = true;
                    }
                    if primary_button(ui, "Backup & Replace").clicked() {
                        confirm = true;
                    }
                });
            });
            if cancel {
                self.replace_shim = false;
            } else if confirm {
                let result = self.install_shim(true);
                self.report(result);
            }
        }
        if let Some(target) = self.pending_switch.clone() {
            let mut discard = false;
            let mut cancel = false;
            if ctx.input(|i| i.key_pressed(Key::Escape)) {
                cancel = true;
            }
            egui::Modal::new(Id::new("discard-changes")).show(ctx, |ui| {
                ui.set_width(max_modal_w.min(420.0));
                ui.label(
                    RichText::new("Discard unsaved changes?")
                        .size(18.0)
                        .strong()
                        .color(TEXT_PRIMARY),
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new(
                        "You have modified the current profile without saving. Switching profiles will discard your edits.",
                    )
                    .size(12.5)
                    .color(TEXT_MUTED),
                );
                ui.add_space(12.0);
                ui.horizontal_wrapped(|ui| {
                    if secondary_button(ui, "Keep Editing").clicked() {
                        cancel = true;
                    }
                    if danger_button(ui, "Discard & Switch").clicked() {
                        discard = true;
                    }
                });
            });
            if cancel {
                self.pending_switch = None;
            } else if discard {
                self.apply_switch(target);
            }
        }
    }
}

fn two_column_row(
    ui: &mut egui::Ui,
    total_width: f32,
    left: impl FnOnce(&mut egui::Ui, f32),
    right: impl FnOnce(&mut egui::Ui, f32),
) {
    let gap = ui.spacing().item_spacing.x;
    let col_w = ((total_width - gap) * 0.5).max(80.0);
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_min_width(col_w);
            ui.set_max_width(col_w);
            left(ui, col_w);
        });
        ui.vertical(|ui| {
            ui.set_min_width(col_w);
            ui.set_max_width(col_w);
            right(ui, col_w);
        });
    });
}

fn full_width_text_edit(
    ui: &mut egui::Ui,
    outer_width: f32,
    value: &mut String,
    hint: &str,
    monospace: bool,
    enabled: bool,
) {
    let desired_inner = (outer_width - 20.0).max(40.0);
    let mut edit = TextEdit::singleline(value)
        .hint_text(hint)
        .desired_width(desired_inner)
        .margin(Margin::symmetric(10, 8));
    if monospace {
        edit = edit.font(TextStyle::Monospace);
    }
    ui.add_enabled(enabled, edit);
}

fn signing_options_ui(ui: &mut egui::Ui, gpg_sign: &mut bool, signing_format: &mut usize) {
    ui.label(
        RichText::new("Commit Signing Options")
            .size(12.5)
            .strong()
            .color(TEXT_PRIMARY),
    );
    ui.add_space(2.0);
    ui.checkbox(gpg_sign, "Sign commits automatically (commit.gpgSign)");
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Format:").size(12.0).color(TEXT_MUTED));
        for (idx, label) in SIGNING_FORMATS.iter().enumerate() {
            ui.selectable_value(signing_format, idx, *label);
        }
    });
}

fn file_picker_row(
    ui: &mut egui::Ui,
    row_w: f32,
    value: &mut String,
    hint: &str,
    button_label: &str,
    button_width: f32,
    dialog_title: &str,
) {
    if file_picker_row_enabled(ui, row_w, true, value, hint, button_label, button_width)
        && let Some(path) = rfd::FileDialog::new().set_title(dialog_title).pick_file()
    {
        *value = path.display().to_string();
    }
}

fn file_picker_row_enabled(
    ui: &mut egui::Ui,
    row_w: f32,
    enabled: bool,
    value: &mut String,
    hint: &str,
    button_label: &str,
    button_width: f32,
) -> bool {
    if row_w >= 440.0 {
        let gap = ui.spacing().item_spacing.x;
        let input_outer_w = (row_w - button_width - gap).max(120.0);
        let mut clicked = false;
        ui.horizontal(|ui| {
            full_width_text_edit(ui, input_outer_w, value, hint, true, enabled);
            if secondary_button_sized(ui, button_label, button_width).clicked() {
                clicked = true;
            }
        });
        clicked
    } else {
        full_width_text_edit(ui, row_w, value, hint, true, enabled);
        secondary_button(ui, button_label).clicked()
    }
}

fn draw_logo(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(8), Color32::from_rgb(32, 38, 48));
    painter.rect_stroke(
        rect,
        CornerRadius::same(8),
        Stroke::new(1.0, BORDER_SUBTLE),
        egui::StrokeKind::Inside,
    );
    let left_top = rect.min + Vec2::new(10.0, 9.0);
    let left_bottom = rect.min + Vec2::new(10.0, 23.0);
    let right_top = rect.min + Vec2::new(22.0, 9.0);
    let mid = rect.min + Vec2::new(10.0, 17.0);
    painter.line_segment([left_top, left_bottom], Stroke::new(2.2, TEXT_PRIMARY));
    painter.line_segment([right_top, mid], Stroke::new(2.2, TEXT_PRIMARY));
    painter.circle_filled(left_top, 3.2, ACCENT_TEAL);
    painter.circle_filled(left_bottom, 3.2, ACCENT_TEAL);
    painter.circle_filled(right_top, 3.2, ACCENT_PURPLE);
}

fn badge(ui: &mut egui::Ui, text: &str, fg: Color32, bg: Color32) {
    Frame::new()
        .fill(bg)
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(8, 3))
        .stroke(Stroke::new(1.0, fg.gamma_multiply(0.45)))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(10.5).strong().color(fg));
        });
}

fn primary_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(
            RichText::new(label)
                .strong()
                .color(Color32::from_rgb(12, 18, 20)),
        )
        .fill(ACCENT_TEAL)
        .stroke(Stroke::new(1.0, ACCENT_TEAL))
        .corner_radius(CornerRadius::same(8)),
    )
}

fn secondary_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(label).color(TEXT_PRIMARY))
            .fill(BG_CARD)
            .stroke(Stroke::new(1.0, BORDER_SUBTLE))
            .corner_radius(CornerRadius::same(8)),
    )
}

fn secondary_button_sized(ui: &mut egui::Ui, label: &str, width: f32) -> egui::Response {
    ui.add_sized(
        [width, 32.0],
        egui::Button::new(RichText::new(label).color(TEXT_PRIMARY))
            .fill(BG_CARD)
            .stroke(Stroke::new(1.0, BORDER_SUBTLE))
            .corner_radius(CornerRadius::same(8)),
    )
}

fn danger_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(label).strong().color(DANGER_RED))
            .fill(DANGER_BG)
            .stroke(Stroke::new(1.0, DANGER_RED.gamma_multiply(0.6)))
            .corner_radius(CornerRadius::same(8)),
    )
}

fn profile_card(
    ui: &mut egui::Ui,
    scope: &Scope,
    title: &str,
    folder_str: &str,
    selected: bool,
    outer_width: f32,
) -> bool {
    let fill = if selected {
        Color32::from_rgb(24, 42, 46)
    } else {
        BG_CARD
    };
    let stroke = if selected {
        Stroke::new(1.5, BORDER_ACTIVE)
    } else {
        Stroke::new(1.0, BORDER_SUBTLE)
    };
    let inner_w = (outer_width - 24.0 - stroke.width * 2.0).max(0.0);
    let response = Frame::new()
        .fill(fill)
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(12, 10))
        .stroke(stroke)
        .show(ui, |ui| {
            ui.set_min_width(inner_w);
            ui.set_max_width(inner_w);
            ui.horizontal_wrapped(|ui| {
                let display_title = if title.is_empty() { folder_str } else { title };
                ui.label(
                    RichText::new(display_title)
                        .size(14.0)
                        .strong()
                        .color(if selected { ACCENT_TEAL } else { TEXT_PRIMARY }),
                );
                let (label, fg, bg) = match scope.auth.as_str() {
                    "personal" => ("GH USER", ACCENT_TEAL, ACCENT_TEAL_DARK),
                    "app" => ("GH APP", ACCENT_PURPLE, ACCENT_PURPLE_DARK),
                    _ => ("GIT ONLY", TEXT_MUTED, BG_INPUT),
                };
                badge(ui, label, fg, bg);
            });
            ui.label(
                RichText::new(folder_str)
                    .font(FontId::new(11.0, FontFamily::Monospace))
                    .color(TEXT_MUTED),
            );
            let identity = match (scope.name.is_empty(), scope.email.is_empty()) {
                (false, false) => format!("{} <{}>", scope.name, scope.email),
                (false, true) => scope.name.clone(),
                (true, false) => scope.email.clone(),
                (true, true) => "No commit author configured".into(),
            };
            ui.label(RichText::new(identity).size(11.5).color(TEXT_PRIMARY));
        })
        .response;

    let interact = ui.interact(
        response.rect,
        ui.make_persistent_id(folder_str),
        Sense::click(),
    );
    if interact.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    interact.clicked()
}

fn section_card(
    ui: &mut egui::Ui,
    outer_width: f32,
    title: &str,
    subtitle: &str,
    add_contents: impl FnOnce(&mut egui::Ui, f32),
) {
    let inner_w = (outer_width - 34.0).max(0.0);
    Frame::new()
        .fill(BG_CARD)
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(16))
        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
        .show(ui, |ui| {
            ui.set_min_width(inner_w);
            ui.set_max_width(inner_w);
            ui.label(RichText::new(title).size(15.0).strong().color(TEXT_PRIMARY));
            ui.label(RichText::new(subtitle).size(12.0).color(TEXT_MUTED));
            ui.add_space(8.0);
            add_contents(ui, inner_w);
        });
}

fn mode_tile(
    ui: &mut egui::Ui,
    outer_width: f32,
    title: &str,
    subtitle: &str,
    active: bool,
) -> bool {
    let fill = if active {
        Color32::from_rgb(24, 42, 46)
    } else {
        BG_INPUT
    };
    let stroke = if active {
        Stroke::new(1.5, ACCENT_TEAL)
    } else {
        Stroke::new(1.0, BORDER_SUBTLE)
    };
    let inner_w = (outer_width - 24.0 - stroke.width * 2.0).max(0.0);
    let response = Frame::new()
        .fill(fill)
        .corner_radius(CornerRadius::same(9))
        .inner_margin(Margin::symmetric(12, 9))
        .stroke(stroke)
        .show(ui, |ui| {
            ui.set_min_width(inner_w);
            ui.set_max_width(inner_w);
            ui.label(RichText::new(title).size(13.5).strong().color(if active {
                ACCENT_TEAL
            } else {
                TEXT_PRIMARY
            }));
            ui.label(RichText::new(subtitle).size(11.5).color(TEXT_MUTED));
        })
        .response;
    let interact = ui.interact(response.rect, ui.make_persistent_id(title), Sense::click());
    if interact.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    interact.clicked()
}

fn field_with_hint(
    ui: &mut egui::Ui,
    outer_width: f32,
    label: &str,
    hint: &str,
    value: &mut String,
    monospace: bool,
) {
    ui.label(RichText::new(label).size(12.5).strong().color(TEXT_PRIMARY));
    full_width_text_edit(ui, outer_width, value, hint, monospace, true);
    ui.add_space(4.0);
}

fn render_diff(ui: &mut egui::Ui, text: &str) {
    ui.spacing_mut().item_spacing.y = 3.0;
    for line in text.lines() {
        let color = if line.starts_with("---") {
            DANGER_RED
        } else if line.starts_with("+++") {
            ACCENT_TEAL
        } else if line == "CURRENT:" || line == "PROPOSED:" {
            ACCENT_PURPLE
        } else {
            TEXT_PRIMARY
        };
        ui.label(
            RichText::new(line)
                .font(FontId::new(12.5, FontFamily::Monospace))
                .color(color),
        );
    }
}

impl eframe::App for Desktop {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.show(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, height),
            )),
            ..Default::default()
        };
        let output = ctx.run(input, |ctx| app.show(ctx));
        assert!(!output.shapes.is_empty());
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
    }
}
