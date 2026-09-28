//! The menu bar under the title bar, drawn and driven here — because egui's
//! own menus know nothing of the keyboard, and a menu bar that cannot be
//! worked from the keyboard is not the one every desktop program has.
//!
//! **What goes where is the registry's.** Each command says which menu it is
//! in (its group), whether a line goes above it, which submenu holds it and
//! whether it is on the menu at all. What is decided here is only what the
//! registry cannot know: the order of the menus, and whether an entry can be
//! chosen right now. A command that cannot be given from where somebody is
//! is greyed rather than left out, so the menus keep their shape.
//!
//! **The keys are the platform's.** On Windows and Linux every entry has an
//! access key, underlined while Alt is held or while the keyboard has the
//! menu: Alt and a letter opens a menu, Alt alone or F10 lights the bar, the
//! arrows walk it, Enter chooses, Escape steps back, and a letter chooses the
//! entry it belongs to. The access keys are worked out, not translated — see
//! [`photosite_core::commands::access_keys`]. The Mac has no access keys and
//! writes its shortcuts in symbols; there the arrows, Enter and Escape work
//! in a menu that is open, and nothing else.
//!
//! While the menu has the keyboard it has all of it: an arrow that walks the
//! menu must not walk the gallery behind it as well.

use crate::App;
use eframe::egui;
use egui::text::LayoutJob;
use egui::{Key, Modifiers, Sense, Vec2};
use photosite_core::commands::{COMMANDS, Command, Group, Menu};
use photosite_core::t;

/// The menus, in the order every program has them. Sorting is a submenu of
/// View rather than a menu of its own.
const MENUS: [Group; 7] = [
    Group::File,
    Group::Edit,
    Group::View,
    Group::Go,
    Group::Photo,
    Group::Editor,
    Group::Help,
];

/// Whether this platform has access keys. The Mac does not: Alt there types
/// letters, and nothing is underlined.
pub const ACCESS_KEYS: bool = !cfg!(target_os = "macos");

/// Where the menu bar stands: which menu is open, what is lit in it, and
/// whether the keyboard has it.
#[derive(Debug, Clone, Default)]
pub struct Bar {
    /// The menu open, by its place along the bar.
    open: Option<usize>,
    /// A menu title lit with no menu open: the keyboard has the bar.
    lit: Option<usize>,
    /// The entry lit in the open menu.
    item: Option<usize>,
    /// Whether the lit entry's submenu is open, and what is lit in it.
    sub_open: bool,
    sub_item: Option<usize>,
    /// The keyboard opened or lit it, so the access keys are shown.
    keyboard: bool,
    /// Alt went down and nothing else has happened since: letting it go
    /// lights the bar.
    alt_alone: bool,
    alt_was_down: bool,
    /// Where each title stood last frame, for its menu to drop from.
    titles: Vec<egui::Rect>,
}

impl Bar {
    /// Whether the menu has the keyboard — a menu open or the bar lit. The
    /// keys that would otherwise walk the gallery are the menu's then.
    pub fn has_keyboard(&self) -> bool {
        self.open.is_some() || self.lit.is_some()
    }

    fn close(&mut self) {
        *self = Bar {
            alt_was_down: self.alt_was_down,
            titles: std::mem::take(&mut self.titles),
            ..Bar::default()
        };
    }

    fn open_menu(&mut self, at: usize, keyboard: bool, menus: &[Model]) {
        self.open = Some(at);
        self.lit = None;
        self.sub_open = false;
        self.sub_item = None;
        self.keyboard = keyboard;
        // From the keyboard the first entry is lit at once, so the arrows
        // have somewhere to start from. From the mouse nothing is, until the
        // pointer is over something.
        self.item = if keyboard {
            menus.get(at).and_then(|menu| step(&menu.rows, None, 1))
        } else {
            None
        };
    }

    /// One key, pressed.
    fn key(
        &mut self,
        key: Key,
        modifiers: Modifiers,
        menus: &[Model],
        run: &mut Option<&'static str>,
    ) {
        let letter = letter_of(key);

        // Alt and a letter opens the menu with that access key, from
        // anywhere. Alt and a letter that is no menu's, with a menu open,
        // is a letter in that menu — Windows has it both ways.
        if ACCESS_KEYS && modifiers.alt && !modifiers.ctrl && !modifiers.command {
            if let Some(letter) = letter {
                if let Some(at) = menus.iter().position(|menu| menu.key == Some(letter)) {
                    self.open_menu(at, true, menus);
                } else if self.open.is_some() {
                    self.choose_letter(letter, menus, run);
                }
            }
            return;
        }

        if !self.has_keyboard() {
            if ACCESS_KEYS && key == Key::F10 && modifiers.is_none() {
                self.lit = Some(0);
                self.keyboard = true;
            }
            return;
        }

        // Anything with Ctrl is a shortcut, and not the menu's to take.
        if modifiers.ctrl || modifiers.command {
            return;
        }

        match self.open {
            None => self.key_on_bar(key, letter, menus),
            Some(at) => self.key_in_menu(at, key, letter, menus, run),
        }
    }

    /// A key while the bar is lit and no menu is open.
    fn key_on_bar(&mut self, key: Key, letter: Option<char>, menus: &[Model]) {
        let count = menus.len().max(1);
        let lit = self.lit.unwrap_or(0);
        match key {
            Key::ArrowRight => self.lit = Some((lit + 1) % count),
            Key::ArrowLeft => self.lit = Some((lit + count - 1) % count),
            Key::ArrowDown | Key::ArrowUp | Key::Enter | Key::Space => {
                self.open_menu(lit, true, menus);
            }
            Key::Escape | Key::F10 => self.close(),
            _ => {
                if let Some(letter) = letter
                    && let Some(at) = menus.iter().position(|menu| menu.key == Some(letter))
                {
                    self.open_menu(at, true, menus);
                }
            }
        }
    }

    /// A key while a menu is open.
    fn key_in_menu(
        &mut self,
        at: usize,
        key: Key,
        letter: Option<char>,
        menus: &[Model],
        run: &mut Option<&'static str>,
    ) {
        let count = menus.len().max(1);
        let Some(menu) = menus.get(at) else {
            self.close();
            return;
        };
        self.keyboard = true;
        let submenu = self
            .item
            .and_then(|item| menu.rows.get(item))
            .and_then(|row| row.sub.as_ref())
            .filter(|_| self.sub_open);

        match (key, submenu) {
            (Key::F10, _) => self.close(),
            (Key::ArrowDown, Some(sub)) => self.sub_item = step(sub, self.sub_item, 1),
            (Key::ArrowUp, Some(sub)) => self.sub_item = step(sub, self.sub_item, -1),
            (Key::Home, Some(sub)) => self.sub_item = step(sub, None, 1),
            (Key::End, Some(sub)) => self.sub_item = step(sub, None, -1),
            (Key::Enter | Key::Space, Some(sub)) => {
                if let Some(row) = self.sub_item.and_then(|item| sub.get(item)) {
                    self.choose(row, run);
                }
            }
            (Key::ArrowLeft | Key::Escape, Some(_)) => {
                self.sub_open = false;
                self.sub_item = None;
            }
            (Key::ArrowRight, Some(_)) => self.open_menu((at + 1) % count, true, menus),
            (Key::ArrowDown, None) => self.item = step(&menu.rows, self.item, 1),
            (Key::ArrowUp, None) => self.item = step(&menu.rows, self.item, -1),
            (Key::Home, None) => self.item = step(&menu.rows, None, 1),
            (Key::End, None) => self.item = step(&menu.rows, None, -1),
            (Key::Enter | Key::Space | Key::ArrowRight, None) => {
                match self.item.and_then(|item| menu.rows.get(item)) {
                    Some(row) if row.sub.is_some() => {
                        self.sub_open = true;
                        self.sub_item = row.sub.as_deref().and_then(|sub| step(sub, None, 1));
                    }
                    Some(row) if key != Key::ArrowRight => self.choose(row, run),
                    _ if key == Key::ArrowRight => self.open_menu((at + 1) % count, true, menus),
                    _ => {}
                }
            }
            (Key::ArrowLeft, None) => self.open_menu((at + count - 1) % count, true, menus),
            // Escape closes the menu and leaves its title lit; a second one
            // gives the keyboard back.
            (Key::Escape, None) => {
                let keyboard = self.keyboard;
                self.close();
                if keyboard && ACCESS_KEYS {
                    self.lit = Some(at);
                    self.keyboard = true;
                }
            }
            _ => {
                if ACCESS_KEYS && let Some(letter) = letter {
                    self.choose_letter(letter, menus, run);
                }
            }
        }
    }

    /// The entry with this access key, in the submenu if one is open.
    fn choose_letter(&mut self, letter: char, menus: &[Model], run: &mut Option<&'static str>) {
        let Some(menu) = self.open.and_then(|at| menus.get(at)) else {
            return;
        };

        let submenu = self
            .item
            .and_then(|item| menu.rows.get(item))
            .and_then(|row| row.sub.as_ref())
            .filter(|_| self.sub_open);
        let rows = submenu.map_or(&menu.rows, |sub| sub);
        let Some(found) = rows.iter().position(|row| row.key == Some(letter)) else {
            return;
        };

        if submenu.is_some() {
            self.sub_item = Some(found);
            self.choose(&rows[found], run);
            return;
        }

        self.item = Some(found);
        match &rows[found].sub {
            Some(sub) => {
                self.sub_open = true;
                self.sub_item = step(sub, None, 1);
            }
            None => self.choose(&rows[found], run),
        }
    }

    /// Chooses an entry: its command runs and the menu closes. A greyed one
    /// does nothing, and the menu stays.
    fn choose(&mut self, row: &Row, run: &mut Option<&'static str>) {
        if let Some(id) = row.command
            && row.enabled
        {
            *run = Some(id);
            self.close();
        }
    }
}

/// One entry of a menu, as it is drawn and chosen this frame.
#[derive(Debug, Clone)]
struct Row {
    /// Nothing for a line between two kinds of entry.
    title: Option<String>,
    /// Where in the title its access key is.
    key_at: Option<usize>,
    key: Option<char>,
    command: Option<&'static str>,
    shortcut: String,
    enabled: bool,
    /// On or off, for a switch; nothing for anything else.
    checked: Option<bool>,
    /// The entries of the submenu it opens.
    sub: Option<Vec<Row>>,
}

impl Row {
    fn line() -> Self {
        Self {
            title: None,
            key_at: None,
            key: None,
            command: None,
            shortcut: String::new(),
            enabled: false,
            checked: None,
            sub: None,
        }
    }

    fn is_line(&self) -> bool {
        self.title.is_none()
    }
}

/// One menu along the bar.
#[derive(Debug, Clone)]
struct Model {
    title: String,
    key_at: Option<usize>,
    key: Option<char>,
    rows: Vec<Row>,
}

/// The menus as they stand this frame, from the registry.
fn menus(app: &App) -> Vec<Model> {
    let titles: Vec<String> = MENUS.iter().map(|group| group.title()).collect();
    // Alt and these letters are commands of their own, so no menu may have
    // them.
    let taken: Vec<char> = COMMANDS
        .iter()
        .filter_map(|command| app.bindings.shortcut(command.id))
        .filter(|shortcut| shortcut.alt && !shortcut.ctrl && !shortcut.shift)
        .filter_map(|shortcut| {
            let mut letters = shortcut.key.chars();
            match (letters.next(), letters.next()) {
                (Some(letter), None) if letter.is_ascii_alphanumeric() => {
                    Some(letter.to_ascii_lowercase())
                }
                _ => None,
            }
        })
        .collect();
    let keys = photosite_core::commands::access_keys(&titles, &taken);

    MENUS
        .iter()
        .zip(titles)
        .zip(keys)
        .map(|((group, title), key_at)| {
            let mut rows = rows_of(app, *group);
            if *group == Group::View {
                rows.push(Row::line());
                let sorting = rows_of(app, Group::Sort);
                rows.push(submenu(t!("menu-sort-by"), sorting));
            }
            with_keys(&mut rows);
            Model {
                key: letter_at(&title, key_at),
                title,
                key_at,
                rows,
            }
        })
        .collect()
}

/// A group's entries, in the registry's order, with its submenus gathered
/// where the first of their commands stands.
fn rows_of(app: &App, group: Group) -> Vec<Row> {
    let mine: Vec<&'static Command> = COMMANDS
        .iter()
        .filter(|command| command.group == group && command.menu != Menu::Hidden)
        .collect();

    let mut rows = Vec::new();
    let mut gathered: Vec<&str> = Vec::new();
    for command in &mine {
        match command.menu {
            Menu::Hidden => {}
            Menu::Item => rows.push(entry(app, command)),
            Menu::Section => {
                if !rows.is_empty() {
                    rows.push(Row::line());
                }
                rows.push(entry(app, command));
            }
            Menu::Under(title) => {
                if gathered.contains(&title) {
                    continue;
                }
                gathered.push(title);
                let under = mine
                    .iter()
                    .filter(|other| other.menu == Menu::Under(title))
                    .map(|other| entry(app, other))
                    .collect();
                rows.push(submenu(photosite_core::i18n::t(title), under));
            }
        }
    }

    rows
}

fn entry(app: &App, command: &Command) -> Row {
    Row {
        title: Some(command.title()),
        key_at: None,
        key: None,
        command: Some(command.id),
        shortcut: app.shortcut_label(command.id).unwrap_or_default(),
        enabled: command.scope.reaches(app.scope()) && app.can_run(command.id),
        checked: app.checked(command.id),
        sub: None,
    }
}

fn submenu(title: String, mut rows: Vec<Row>) -> Row {
    with_keys(&mut rows);
    Row {
        title: Some(title),
        key_at: None,
        key: None,
        command: None,
        shortcut: String::new(),
        enabled: true,
        checked: None,
        sub: Some(rows),
    }
}

/// Access keys for one menu's entries, none shared within it.
fn with_keys(rows: &mut [Row]) {
    let titles: Vec<String> = rows
        .iter()
        .map(|row| row.title.clone().unwrap_or_default())
        .collect();
    for (row, at) in rows
        .iter_mut()
        .zip(photosite_core::commands::access_keys(&titles, &[]))
    {
        row.key_at = at;
        row.key = row.title.as_deref().and_then(|title| letter_at(title, at));
    }
}

fn letter_at(title: &str, at: Option<usize>) -> Option<char> {
    title
        .chars()
        .nth(at?)
        .map(|letter| letter.to_ascii_lowercase())
}

/// The letter or digit a key types, as an access key is written.
fn letter_of(key: Key) -> Option<char> {
    let mut name = key.name().chars();
    match (name.next(), name.next()) {
        (Some(letter), None) if letter.is_ascii_alphanumeric() => Some(letter.to_ascii_lowercase()),
        _ => None,
    }
}

/// The next entry that is not a line, round from the end to the start.
/// From nothing, the first — or, going back, the last.
fn step(rows: &[Row], from: Option<usize>, by: isize) -> Option<usize> {
    let count = rows.len() as isize;
    if count == 0 {
        return None;
    }

    let mut at = match from {
        Some(from) => from as isize,
        None if by > 0 => -1,
        None => count,
    };
    for _ in 0..count {
        at = (at + by).rem_euclid(count);
        if !rows[at as usize].is_line() {
            return Some(at as usize);
        }
    }

    None
}

/// Reads the keys before anything else sees them. Called before the
/// application's own shortcuts every frame: while the menu has the keyboard,
/// it has all of it.
pub fn keys(app: &mut App, ctx: &egui::Context) {
    let menus = menus(app);
    let mut bar = std::mem::take(&mut app.bar);
    let (events, modifiers) = ctx.input(|input| (input.events.clone(), input.modifiers));
    let mut run: Option<&'static str> = None;

    // Alt alone, pressed and let go with nothing in between, lights the bar
    // — and puts it out again.
    if ACCESS_KEYS {
        let something_else = events.iter().any(|event| {
            matches!(
                event,
                egui::Event::Key { pressed: true, .. } | egui::Event::PointerButton { .. }
            )
        });
        if modifiers.alt && !bar.alt_was_down {
            bar.alt_alone = true;
        }
        if modifiers.alt && (something_else || modifiers.ctrl || modifiers.shift) {
            bar.alt_alone = false;
        }
        if !modifiers.alt && bar.alt_was_down && bar.alt_alone {
            if bar.has_keyboard() {
                bar.close();
            } else {
                bar.lit = Some(0);
                bar.keyboard = true;
            }
        }
        if !modifiers.alt {
            bar.alt_alone = false;
        }
        bar.alt_was_down = modifiers.alt;
    }

    for event in &events {
        if let egui::Event::Key {
            key,
            pressed: true,
            modifiers,
            ..
        } = event
        {
            bar.key(*key, *modifiers, &menus, &mut run);
        }
    }

    if bar.has_keyboard() {
        ctx.input_mut(|input| {
            input.events.retain(|event| {
                !matches!(
                    event,
                    egui::Event::Key { .. }
                        | egui::Event::Text(_)
                        | egui::Event::Copy
                        | egui::Event::Cut
                        | egui::Event::Paste(_)
                )
            });
        });
    }

    app.bar = bar;
    if let Some(id) = run {
        app.run(id, ctx);
    }
}

/// The bar, and the menu open from it.
pub fn bar(app: &mut App, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let menus = menus(app);
    let mut bar = std::mem::take(&mut app.bar);
    let underline = ACCESS_KEYS && (bar.keyboard || ui.input(|input| input.modifiers.alt));
    let mut run: Option<&'static str> = None;

    bar.titles.clear();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for (at, menu) in menus.iter().enumerate() {
            let lit = bar.open == Some(at) || bar.lit == Some(at);
            let text = labelled(
                ui,
                &menu.title,
                menu.key_at.filter(|_| underline),
                ui.visuals().text_color(),
            );
            let response = ui.add(egui::Button::selectable(lit, egui::WidgetText::from(text)));
            bar.titles.push(response.rect);
            if response.clicked() {
                if bar.open == Some(at) {
                    bar.close();
                } else {
                    bar.open_menu(at, false, &menus);
                }
            } else if response.hovered() && bar.open.is_some() && bar.open != Some(at) {
                // Moving along the bar with a menu open opens the next one,
                // as it does everywhere.
                bar.open_menu(at, false, &menus);
            }
        }
    });

    let mut inside: Vec<egui::Rect> = bar.titles.clone();
    if let Some(at) = bar.open
        && let (Some(menu), Some(title)) = (menus.get(at), bar.titles.get(at).copied())
    {
        let pointer_moved = ui.input(|input| input.pointer.delta() != Vec2::ZERO);
        let mut sub_at: Option<(egui::Pos2, usize)> = None;
        let dropped = egui::Area::new(egui::Id::new("menu bar"))
            .order(egui::Order::Foreground)
            .fixed_pos(title.left_bottom())
            .constrain(true)
            .show(&ctx, |ui| {
                egui::Frame::menu(ui.style()).show(ui, |ui| {
                    let width = width_of(ui, &menu.rows);
                    for (index, row) in menu.rows.iter().enumerate() {
                        if row.is_line() {
                            line(ui, width);
                            continue;
                        }

                        let lit = bar.item == Some(index);
                        let response = draw_row(ui, row, width, lit, underline);
                        if response.hovered() && pointer_moved {
                            if bar.item != Some(index) {
                                bar.sub_item = None;
                            }
                            bar.item = Some(index);
                            bar.sub_open = row.sub.is_some();
                        }
                        if response.clicked() {
                            bar.item = Some(index);
                            match &row.sub {
                                Some(_) => bar.sub_open = true,
                                None => bar.choose(row, &mut run),
                            }
                        }
                        if row.sub.is_some() && bar.sub_open && bar.item == Some(index) {
                            sub_at = Some((response.rect.right_top(), index));
                        }
                    }
                });
            });
        inside.push(dropped.response.rect);

        if let Some((corner, index)) = sub_at
            && let Some(sub) = menu.rows[index].sub.as_ref()
        {
            let shown = egui::Area::new(egui::Id::new("menu bar submenu"))
                .order(egui::Order::Foreground)
                .fixed_pos(corner + Vec2::new(4.0, -4.0))
                .constrain(true)
                .show(&ctx, |ui| {
                    egui::Frame::menu(ui.style()).show(ui, |ui| {
                        let width = width_of(ui, sub);
                        for (at, row) in sub.iter().enumerate() {
                            if row.is_line() {
                                line(ui, width);
                                continue;
                            }

                            let lit = bar.sub_item == Some(at);
                            let response = draw_row(ui, row, width, lit, underline);
                            if response.hovered() && pointer_moved {
                                bar.sub_item = Some(at);
                            }
                            if response.clicked() {
                                bar.choose(row, &mut run);
                            }
                        }
                    });
                });
            inside.push(shown.response.rect);
        }
    }

    // A click anywhere else puts the menu away, and is not also a click on
    // whatever was under it.
    let pressed_outside = ui.input(|input| {
        input.pointer.any_pressed()
            && input
                .pointer
                .interact_pos()
                .is_some_and(|at| !inside.iter().any(|rect| rect.contains(at)))
    });
    if pressed_outside && bar.has_keyboard() {
        bar.close();
    }

    app.bar = bar;
    if let Some(id) = run {
        app.run(id, &ctx);
    }
}

/// A title with its access key underlined, when it is to be.
fn labelled(ui: &egui::Ui, title: &str, key_at: Option<usize>, color: egui::Color32) -> LayoutJob {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let plain = egui::TextFormat {
        font_id: font,
        color,
        ..Default::default()
    };

    let mut job = LayoutJob::default();
    let letters: Vec<char> = title.chars().collect();
    match key_at.filter(|at| *at < letters.len()) {
        Some(at) => {
            let before: String = letters[..at].iter().collect();
            let after: String = letters[at + 1..].iter().collect();
            job.append(&before, 0.0, plain.clone());
            job.append(
                &letters[at].to_string(),
                0.0,
                egui::TextFormat {
                    underline: egui::Stroke::new(1.0, color),
                    ..plain.clone()
                },
            );
            job.append(&after, 0.0, plain);
        }
        None => job.append(title, 0.0, plain),
    }
    job
}

/// Room for the tick, the longest title, the longest key and the arrow.
fn width_of(ui: &egui::Ui, rows: &[Row]) -> f32 {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let color = ui.visuals().text_color();
    let measure = |text: &str| {
        ui.painter()
            .layout_no_wrap(text.to_owned(), font.clone(), color)
            .size()
            .x
    };
    let title = rows
        .iter()
        .filter_map(|row| row.title.as_deref())
        .map(measure)
        .fold(0.0, f32::max);
    let shortcut = rows
        .iter()
        .map(|row| measure(&row.shortcut))
        .fold(0.0, f32::max);
    (TICK + title + 32.0 + shortcut + ARROW).max(200.0)
}

/// The column the tick stands in, and the one the arrow does.
const TICK: f32 = 24.0;
const ARROW: f32 = 22.0;

fn line(ui: &mut egui::Ui, width: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 7.0), Sense::hover());
    ui.painter().hline(
        rect.x_range().shrink(4.0),
        rect.center().y,
        ui.visuals().widgets.noninteractive.bg_stroke,
    );
}

/// One entry, drawn: lit or not, greyed or not, with its tick, its title,
/// its key and, for a submenu, its arrow. Painted rather than put together
/// out of widgets, so that every part stands in its own column.
fn draw_row(
    ui: &mut egui::Ui,
    row: &Row,
    width: f32,
    lit: bool,
    underline: bool,
) -> egui::Response {
    let height = ui.spacing().interact_size.y + 2.0;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    let visuals = ui.visuals().clone();
    let painter = ui.painter();

    if lit {
        let fill = if row.enabled {
            visuals.selection.bg_fill
        } else {
            visuals.widgets.hovered.weak_bg_fill
        };
        painter.rect_filled(rect, 2.0, fill);
    }

    let color = match (row.enabled, lit) {
        (false, _) => visuals.weak_text_color(),
        (true, true) => visuals.selection.stroke.color,
        (true, false) => visuals.text_color(),
    };

    // A tick drawn rather than written: the default font has none.
    if row.checked == Some(true) {
        let middle = egui::pos2(rect.left() + TICK * 0.5, rect.center().y);
        painter.line(
            vec![
                middle + Vec2::new(-4.0, 0.0),
                middle + Vec2::new(-1.0, 3.5),
                middle + Vec2::new(5.0, -4.0),
            ],
            egui::Stroke::new(1.6, color),
        );
    }

    let title = labelled(
        ui,
        row.title.as_deref().unwrap_or_default(),
        row.key_at.filter(|_| underline),
        color,
    );
    let galley = painter.layout_job(title);
    painter.galley(
        egui::pos2(rect.left() + TICK, rect.center().y - galley.size().y * 0.5),
        galley,
        color,
    );

    if row.sub.is_some() {
        // A small triangle pointing to where the submenu opens.
        let tip = egui::pos2(rect.right() - 9.0, rect.center().y);
        painter.add(egui::Shape::convex_polygon(
            vec![tip, tip + Vec2::new(-5.0, -4.5), tip + Vec2::new(-5.0, 4.5)],
            color,
            egui::Stroke::NONE,
        ));
    } else if !row.shortcut.is_empty() {
        let dim = if lit {
            color
        } else {
            visuals.weak_text_color()
        };
        painter.text(
            egui::pos2(rect.right() - ARROW, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            &row.shortcut,
            egui::TextStyle::Button.resolve(ui.style()),
            dim,
        );
    }

    response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(titles: &[&str]) -> Vec<Row> {
        titles
            .iter()
            .map(|title| match *title {
                "-" => Row::line(),
                title => Row {
                    title: Some(title.to_owned()),
                    ..Row::line()
                },
            })
            .collect()
    }

    /// The arrows go round, and never stop on a line.
    #[test]
    fn the_arrows_step_over_the_lines_and_go_round() {
        let rows = rows(&["Open", "-", "Save", "Quit"]);
        assert_eq!(step(&rows, None, 1), Some(0));
        assert_eq!(step(&rows, Some(0), 1), Some(2), "it stopped on the line");
        assert_eq!(step(&rows, Some(3), 1), Some(0), "it did not go round");
        assert_eq!(step(&rows, None, -1), Some(3));
        assert_eq!(step(&rows, Some(2), -1), Some(0));
        assert_eq!(step(&[], None, 1), None);
    }

    /// The keyboard, which is Windows' and Linux's way: the Mac has no
    /// access keys and no Alt to light the bar with.
    #[cfg(not(target_os = "macos"))]
    mod keyboard {
        use super::super::*;

        /// A frame the way the window has one: the menu reads the keys first,
        /// then the application's own shortcuts, then the bar is drawn.
        fn a_frame(
            app: &mut App,
            ctx: &egui::Context,
            modifiers: Modifiers,
            pressed: &[(Key, Modifiers)],
        ) -> egui::FullOutput {
            // What is held comes first, the way the window says so before the
            // key that goes with it.
            let events = std::iter::once(egui::Event::ModifiersChanged(modifiers))
                .chain(pressed.iter().map(|(key, modifiers)| egui::Event::Key {
                    key: *key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: *modifiers,
                }))
                .collect();
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(0.0, 0.0),
                    egui::vec2(1200.0, 800.0),
                )),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                let ctx = ui.ctx().clone();
                keys(app, &ctx);
                app.shortcuts(&ctx);
                bar(app, ui);
            });
            out.textures_delta.clear();
            out
        }

        fn press(app: &mut App, ctx: &egui::Context, key: Key) {
            a_frame(app, ctx, Modifiers::NONE, &[(key, Modifiers::NONE)]);
        }

        fn alt() -> Modifiers {
            Modifiers {
                alt: true,
                ..Modifiers::NONE
            }
        }

        #[test]
        fn alt_alone_lights_the_bar_and_escape_puts_it_out() {
            let (mut app, _data, _photos) = crate::culling::three();
            let ctx = egui::Context::default();
            a_frame(&mut app, &ctx, alt(), &[]);
            a_frame(&mut app, &ctx, Modifiers::NONE, &[]);
            assert_eq!(app.bar.lit, Some(0), "Alt let go did not light File");
            assert!(app.bar.has_keyboard());

            press(&mut app, &ctx, Key::Escape);
            assert!(!app.bar.has_keyboard());
        }

        /// Alt and a key that is also a command, Alt+C, is the command and not
        /// the bar: Alt was not alone.
        #[test]
        fn alt_with_something_else_is_not_alt_alone() {
            let (mut app, _data, _photos) = crate::culling::three();
            let ctx = egui::Context::default();
            a_frame(&mut app, &ctx, alt(), &[(Key::Z, alt())]);
            a_frame(&mut app, &ctx, Modifiers::NONE, &[]);
            assert!(!app.bar.has_keyboard());
        }

        #[test]
        fn f10_and_the_arrows_walk_the_bar_and_the_menus() {
            let (mut app, _data, _photos) = crate::culling::three();
            let ctx = egui::Context::default();
            press(&mut app, &ctx, Key::F10);
            assert_eq!(app.bar.lit, Some(0));

            press(&mut app, &ctx, Key::ArrowRight);
            assert_eq!(app.bar.lit, Some(1));

            press(&mut app, &ctx, Key::ArrowDown);
            assert_eq!(app.bar.open, Some(1), "Down did not open Edit");
            assert_eq!(app.bar.item, Some(0), "nothing is lit in it");

            press(&mut app, &ctx, Key::ArrowDown);
            assert_eq!(app.bar.item, Some(1));

            press(&mut app, &ctx, Key::ArrowLeft);
            assert_eq!(app.bar.open, Some(0), "Left did not move to File");

            press(&mut app, &ctx, Key::Escape);
            assert_eq!(app.bar.open, None);
            assert_eq!(app.bar.lit, Some(0), "Escape went all the way out at once");

            press(&mut app, &ctx, Key::Escape);
            assert!(!app.bar.has_keyboard());
        }

        /// Alt+V opens View; the letter underlined beside the list view switches
        /// it, and the menu goes.
        #[test]
        fn alt_and_a_letter_opens_a_menu_and_a_letter_chooses_in_it() {
            let (mut app, _data, _photos) = crate::culling::three();
            let ctx = egui::Context::default();
            let was = app.settings.gallery.as_list;

            a_frame(&mut app, &ctx, alt(), &[(Key::V, alt())]);
            assert_eq!(app.bar.open, Some(2), "Alt+V did not open View");

            let letter = menus(&app)[2]
                .rows
                .iter()
                .find(|row| row.command == Some("view.as_list"))
                .and_then(|row| row.key)
                .expect("the list view has no access key");
            let key = Key::from_name(&letter.to_ascii_uppercase().to_string()).unwrap();
            a_frame(&mut app, &ctx, Modifiers::NONE, &[(key, Modifiers::NONE)]);

            assert_ne!(
                app.settings.gallery.as_list, was,
                "the letter chose nothing"
            );
            assert!(!app.bar.has_keyboard(), "the menu stayed open");
        }

        /// While the menu has the keyboard, an arrow walks the menu and not the
        /// gallery behind it.
        #[test]
        fn the_arrows_in_a_menu_do_not_walk_the_gallery() {
            let (mut app, _data, _photos) = crate::culling::three();
            let ctx = egui::Context::default();
            app.select_only(0);
            a_frame(&mut app, &ctx, alt(), &[(Key::F, alt())]);
            press(&mut app, &ctx, Key::ArrowRight);
            press(&mut app, &ctx, Key::ArrowDown);
            assert_eq!(app.selected, Some(0), "the gallery moved under the menu");
        }

        /// The access keys are underlined once the keyboard has the menu, and
        /// not before.
        #[test]
        fn the_access_keys_are_underlined_when_the_keyboard_has_the_menu() {
            let (mut app, _data, _photos) = crate::culling::three();
            let ctx = egui::Context::default();
            let underlined = |out: &egui::FullOutput| {
                out.shapes.iter().any(|clipped| {
                    let mut stack = vec![&clipped.shape];
                    let mut found = false;
                    while let Some(shape) = stack.pop() {
                        match shape {
                            egui::Shape::Text(text) => {
                                found |= text
                                    .galley
                                    .job
                                    .sections
                                    .iter()
                                    .any(|section| section.format.underline.width > 0.0);
                            }
                            egui::Shape::Vec(shapes) => stack.extend(shapes.iter()),
                            _ => {}
                        }
                    }
                    found
                })
            };

            let out = a_frame(&mut app, &ctx, Modifiers::NONE, &[]);
            assert!(!underlined(&out), "underlined before anybody asked");

            a_frame(
                &mut app,
                &ctx,
                Modifiers::NONE,
                &[(Key::F10, Modifiers::NONE)],
            );
            let out = a_frame(&mut app, &ctx, Modifiers::NONE, &[]);
            assert!(underlined(&out));
        }
    }

    /// Every entry of every menu can be reached from the keyboard: none is
    /// left without a letter of its own.
    #[test]
    fn every_entry_has_an_access_key() {
        let (app, _data, _photos) = crate::culling::three();
        let mut missing = Vec::new();
        for menu in menus(&app) {
            assert!(menu.key.is_some(), "{} has no letter", menu.title);
            let mut rows: Vec<&Row> = menu.rows.iter().collect();
            while let Some(row) = rows.pop() {
                if let Some(sub) = &row.sub {
                    rows.extend(sub.iter());
                }
                if !row.is_line() && row.key.is_none() {
                    missing.push(format!("{} > {}", menu.title, row.title.clone().unwrap()));
                }
            }
        }
        assert!(missing.is_empty(), "{missing:#?}");
    }

    #[test]
    fn a_key_types_the_letter_it_is_for() {
        assert_eq!(letter_of(Key::F), Some('f'));
        assert_eq!(letter_of(Key::Num3), Some('3'));
        assert_eq!(letter_of(Key::F10), None);
        assert_eq!(letter_of(Key::ArrowLeft), None);
    }
}
