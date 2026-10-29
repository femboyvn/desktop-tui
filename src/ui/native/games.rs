mod snake;
mod puzzle2048;
mod minesweeper;

use crate::config::catalog::GameKind;
use appcui::prelude::*;
use snake::Snake;
use puzzle2048::Puzzle2048;
use minesweeper::Minesweeper;
use std::hash::BuildHasher;
use std::time::{Duration, SystemTime};

enum Game { Snake(Snake), Puzzle2048(Puzzle2048), Minesweeper(Minesweeper) }
impl Game {
    fn new(kind: GameKind, seed: u64) -> Self {
        match kind { GameKind::Snake => Self::Snake(Snake::new(seed)), GameKind::Puzzle2048 => Self::Puzzle2048(Puzzle2048::new(seed)), GameKind::Minesweeper => Self::Minesweeper(Minesweeper::new(seed)) }
    }
    fn phase(&self) -> Phase {
        match self { Self::Snake(game) => game.phase, Self::Puzzle2048(game) => game.phase, Self::Minesweeper(game) => game.phase }
    }
    fn score(&self) -> u64 {
        match self { Self::Snake(game) => u64::from(game.score), Self::Puzzle2048(game) => game.score, Self::Minesweeper(game) => game.cells.iter().filter(|cell| cell.revealed && !cell.mine).count() as u64 }
    }
}

#[CustomControl(overwrite = OnPaint+OnKeyPressed+OnMouseEvent+OnFocus+OnResize)]
struct GameControl { kind: GameKind, game: Game, seed: u64, best: u64, cursor: usize, viewport: Size }
impl GameControl {
    fn new(kind: GameKind, seed: u64) -> Self {
        Self { base: ControlBase::new(Layout::fill(), true), kind, game: Game::new(kind, seed), seed, best: 0, cursor: 40, viewport: Size::new(0, 0) }
    }
    fn input(&mut self, key: Key, character: char) -> EventProcessStatus {
        if self.origin(self.viewport).is_none() || (key.modifier != KeyModifier::None && key.modifier != KeyModifier::Shift) {
            return EventProcessStatus::Ignored;
        }
        if matches!(character, 'r' | 'R') {
            self.best = self.best.max(self.game.score());
            self.seed = self.seed.wrapping_add(0x9e3779b97f4a7c15);
            self.game = Game::new(self.kind, self.seed);
            self.cursor = 40;
            return EventProcessStatus::Processed;
        }
        let direction = match (key.code, character) {
            (KeyCode::Up, _) | (_, 'w' | 'W') => Some(Direction::Up),
            (KeyCode::Right, _) | (_, 'd' | 'D') => Some(Direction::Right),
            (KeyCode::Down, _) | (_, 's' | 'S') => Some(Direction::Down),
            (KeyCode::Left, _) | (_, 'a' | 'A') => Some(Direction::Left),
            _ => None,
        };
        match &mut self.game {
            Game::Snake(game) => {
                if let Some(direction) = direction {
                    if game.phase == Phase::Ready { game.phase = Phase::Playing; }
                    if game.phase == Phase::Playing { game.turn(direction); }
                } else if key.code == KeyCode::Space || matches!(character, ' ' | 'p' | 'P') {
                    game.phase = match game.phase {
                        Phase::Ready | Phase::Paused => Phase::Playing,
                        Phase::Playing => Phase::Paused,
                        other => other,
                    };
                } else { return EventProcessStatus::Ignored; }
            }
            Game::Puzzle2048(game) => {
                let Some(direction) = direction else { return EventProcessStatus::Ignored; };
                game.slide(direction);
            }
            Game::Minesweeper(game) => {
                if let Some(direction) = direction {
                    let (dx, dy) = direction.delta();
                    let x = (self.cursor as i32 % 9 + dx).clamp(0, 8);
                    let y = (self.cursor as i32 / 9 + dy).clamp(0, 8);
                    self.cursor = (y * 9 + x) as usize;
                } else if matches!(key.code, KeyCode::Space | KeyCode::Enter) || character == ' ' { game.reveal(self.cursor); }
                else if matches!(character, 'f' | 'F') { game.flag(self.cursor); }
                else { return EventProcessStatus::Ignored; }
            }
        }
        self.best = self.best.max(self.game.score());
        EventProcessStatus::Processed
    }
    fn advance(&mut self) -> bool {
        if self.origin(self.viewport).is_none() { self.pause(); return false; }
        if let Game::Snake(game) = &mut self.game && game.phase == Phase::Playing {
            game.step();
            self.best = self.best.max(self.game.score());
            return true;
        }
        false
    }
    fn pause(&mut self) {
        if let Game::Snake(game) = &mut self.game && game.phase == Phase::Playing { game.phase = Phase::Paused; }
    }
    fn board_size(&self) -> (u32, u32) {
        match self.kind { GameKind::Snake => (42, 14), GameKind::Puzzle2048 => (29, 13), GameKind::Minesweeper => (29, 11) }
    }
    fn origin(&self, size: Size) -> Option<(i32, i32)> {
        let (width, height) = self.board_size();
        (size.width >= width && size.height >= height + 6).then(|| (((size.width - width) / 2) as i32, 3))
    }
}
impl OnPaint for GameControl {
    fn on_paint(&self, surface: &mut Surface, theme: &Theme) {
        surface.clear(Character::with_attributes(' ', theme.window.normal));
        // AppCUI paints into the shared terminal surface with a control clip.
        // Its surface dimensions are not the game window's dimensions.
        let Some((left, top)) = self.origin(self.viewport) else {
            let (width, height) = self.board_size();
            surface.write_string(0, 0, "Resize window to play", theme.text.normal, false);
            surface.write_string(0, 2, &format!("Needs {} x {} including borders", width + 2, height + 8), theme.text.normal, false);
            return;
        };
        let summary = match &self.game {
            Game::Minesweeper(game) => format!("{} mines  {} flags  {}/71 safe", minesweeper::MINES,
                game.cells.iter().filter(|cell| cell.flagged).count(), self.game.score()),
            _ => format!("Score: {}   Best: {}", self.game.score(), self.best),
        };
        surface.write_string(left, 0, &summary, theme.text.normal, false);
        let message = match self.game.phase() {
            Phase::Ready if self.kind == GameKind::Snake => "Arrows / WASD / Space to start",
            Phase::Ready => "First reveal is safe",
            Phase::Playing => match self.kind {
                GameKind::Snake => "Eat * and avoid walls and your tail",
                GameKind::Puzzle2048 => "Merge tiles to reach 2048",
                GameKind::Minesweeper => "Reveal every safe cell to win",
            },
            Phase::Paused => "Paused - Space / P to resume",
            Phase::Lost => "Game over - R to try again",
            Phase::Won => "You won! R to play again",
        };
        let attr = if self.game.phase() == Phase::Lost { theme.text.error } else { theme.text.normal };
        surface.write_string(left, 1, message, attr, false);
        let (width, height) = self.board_size();
        surface.draw_rect(Rect::new(left, top, left + width as i32 - 1, top + height as i32 - 1), LineType::Single, theme.lines.normal);
        match &self.game {
            Game::Snake(game) => {
                if let Some((x, y)) = game.food {
                    surface.write_string(left + 1 + x * 2, top + 1 + y, "* ", theme.text.warning, false);
                }
                for (index, &(x, y)) in game.body.iter().enumerate() {
                    surface.write_string(left + 1 + x * 2, top + 1 + y, if index == 0 { "██" } else { "▓▓" }, theme.text.enphasized_1, false);
                }
            }
            Game::Puzzle2048(game) => {
                for row in 0..4 { for column in 0..4 {
                    let (x, y) = (left + column * 7, top + row * 3);
                    surface.draw_rect(Rect::new(x, y, x + 7, y + 3), LineType::Single, theme.lines.normal);
                    let value = game.board[(row * 4 + column) as usize];
                    if value != 0 {
                        let text = value.to_string();
                        let attr = match value { 2 | 4 => theme.text.normal, 8 | 16 | 32 => theme.text.enphasized_1, _ => theme.text.enphasized_2 };
                        surface.write_string(x + 1 + (6 - text.len() as i32) / 2, y + 1, &text, attr, false);
                    }
                } }
            }
            Game::Minesweeper(game) => {
                for (index, cell) in game.cells.iter().enumerate() {
                    let symbol = if cell.mine && matches!(game.phase, Phase::Lost | Phase::Won) { '*' }
                        else if cell.flagged { 'F' } else if !cell.revealed { '#' }
                        else { let count = game.adjacent(index); if count == 0 { ' ' } else { char::from(b'0' + count as u8) } };
                    let attr = if index == self.cursor { theme.text.highlighted }
                        else if symbol == '*' { theme.text.error } else if cell.flagged { theme.text.warning }
                        else if cell.revealed { theme.text.normal } else { theme.text.enphasized_1 };
                    let text = format!(" {symbol} ");
                    surface.write_string(left + 1 + (index % 9) as i32 * 3, top + 1 + (index / 9) as i32, &text, attr, false);
                }
            }
        }
        let help: &[&str] = match self.kind {
            GameKind::Snake => &["Arrows / WASD: steer", "Space / P: pause   R: restart"],
            GameKind::Puzzle2048 => &["Arrows / WASD: slide", "R: new game"],
            GameKind::Minesweeper => &["Arrows: select  Enter: reveal", "F: flag  L/R click  R: new"],
        };
        for (row, text) in help.iter().enumerate() {
            surface.write_string(left, top + height as i32 + 1 + row as i32, text, theme.text.normal, false);
        }
    }
}
impl OnKeyPressed for GameControl {
    fn on_key_pressed(&mut self, key: Key, character: char) -> EventProcessStatus {
        if !self.has_focus() { return EventProcessStatus::Ignored; }
        self.input(key, character)
    }
}
impl OnMouseEvent for GameControl {
    fn on_mouse_event(&mut self, event: &MouseEvent) -> EventProcessStatus {
        let MouseEvent::Pressed(data) = event else { return EventProcessStatus::Ignored; };
        if !matches!(data.button, MouseButton::Left | MouseButton::Right) || data.modifier != KeyModifier::None {
            return EventProcessStatus::Ignored;
        }
        self.request_focus();
        let Some((left, top)) = self.origin(self.viewport) else { return EventProcessStatus::Ignored; };
        let Game::Minesweeper(game) = &mut self.game else { return EventProcessStatus::Ignored; };
        let (x, y) = (data.x - left - 1, data.y - top - 1);
        if !(0..27).contains(&x) || !(0..9).contains(&y) { return EventProcessStatus::Ignored; }
        self.cursor = (y * 9 + x / 3) as usize;
        if data.button == MouseButton::Left { game.reveal(self.cursor); } else { game.flag(self.cursor); }
        EventProcessStatus::Processed
    }
}
impl OnFocus for GameControl { fn on_lose_focus(&mut self) { self.pause(); } }
impl OnResize for GameControl {
    fn on_resize(&mut self, _: Size, size: Size) {
        self.viewport = size;
        if self.origin(size).is_none() { self.pause(); }
    }
}

#[Window(events = TimerEvents+WindowEvents)]
pub(crate) struct GameWindow { game: Handle<GameControl>, closing: bool }
impl GameWindow {
    pub fn new(kind: GameKind) -> anyhow::Result<Self> {
        let seed = std::collections::hash_map::RandomState::new().hash_one(SystemTime::now());
        let mut window = Self {
            base: Window::new(kind.name(), layout!("a:c,w:66,h:22"), window::Flags::Sizeable),
            game: Handle::None, closing: false,
        };
        window.game = window.add(GameControl::new(kind, seed));
        window.base.request_focus_for_control(window.game);
        if kind == GameKind::Snake {
            window.timer().ok_or_else(|| anyhow::anyhow!("Snake timer unavailable"))?.start(Duration::from_millis(140));
        }
        Ok(window)
    }
    pub fn is_closing(&self) -> bool { self.closing }
    pub fn constrain_to(&mut self, available: Size) {
        let width = available.width.clamp(1, u16::MAX as u32) as u16;
        let height = available.height.clamp(1, u16::MAX as u32) as u16;
        let size = self.size();
        let position = self.position();
        self.set_size_bounds(1, 1, width, height);
        // Keep moved windows reachable after the terminal shrinks. Before a
        // new window's first layout, retain its requested size and centering.
        if size.width > 1 && size.height > 1 {
            let x = position.x.clamp(0, i32::from(width) - size.width.min(u32::from(width)) as i32);
            let y = position.y.clamp(0, i32::from(height) - size.height.min(u32::from(height)) as i32);
            if x != position.x || y != position.y { self.set_position(x, y); }
        }
        let game = self.game;
        if let Some(control) = self.control_mut(game)
            && control.origin(Size::new(u32::from(width).saturating_sub(2), u32::from(height).saturating_sub(2))).is_none() {
            control.pause();
        }
    }
    pub fn suspend(&mut self) { let game = self.game; if let Some(control) = self.control_mut(game) { control.pause(); } }
    pub fn shutdown(&mut self) { self.closing = true; self.suspend(); self.close(); }
}
impl TimerEvents for GameWindow {
    fn on_update(&mut self, _: u64) -> EventProcessStatus {
        if self.closing || !self.is_visible() || !self.has_focus() { self.suspend(); return EventProcessStatus::Ignored; }
        let game = self.game;
        if self.control_mut(game).is_some_and(GameControl::advance) { EventProcessStatus::Processed }
        else { EventProcessStatus::Ignored }
    }
}
impl WindowEvents for GameWindow {
    fn on_cancel(&mut self) -> ActionRequest { self.closing = true; self.suspend(); ActionRequest::Allow }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Direction { Up, Right, Down, Left }
impl Direction {
    fn delta(self) -> (i32, i32) {
        match self { Self::Up => (0, -1), Self::Right => (1, 0), Self::Down => (0, 1), Self::Left => (-1, 0) }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase { Ready, Playing, Paused, Lost, Won }

// Small local PRNG for game layouts, independent of process-wide random state.
struct Random(u64);
impl Random {
    fn new(seed: u64) -> Self { Self(seed.max(1)) }
    fn below(&mut self, limit: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % limit as u64) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::UI_LOCK;
    use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
    #[Desktop(events = DesktopEvents, overwrite = OnResize)]
    struct ResizeProbe { window: Handle<GameWindow>, kind: GameKind, observed: Arc<AtomicBool> }
    impl DesktopEvents for ResizeProbe {
        fn on_start(&mut self) {
            let mut window = GameWindow::new(self.kind).unwrap();
            let handle = window.game;
            if let Game::Snake(game) = &mut window.control_mut(handle).unwrap().game { game.phase = Phase::Playing; }
            window.constrain_to(self.desktop_rect().size());
            self.window = self.add_window(window);
        }
    }
    impl OnResize for ResizeProbe {
        fn on_resize(&mut self, _: Size, size: Size) {
            let handle = self.window;
            let kind = self.kind;
            let Some(window) = self.window_mut(handle) else { return; };
            window.constrain_to(size);
            let game = window.game;
            let control = window.control_mut(game).unwrap();
            if size.width == 30 && kind == GameKind::Snake {
                assert_eq!(control.game.phase(), Phase::Paused, "shrinking an active Snake must pause it");
            }
            if size.width == 80 {
                // The previous small layout is still present before this
                // resize callback's new bounds are applied by AppCUI.
                assert!(control.origin(control.viewport).is_none());
                assert!(matches!(control.input(key!("Right").into(), '\0'), EventProcessStatus::Ignored));
                assert!(matches!(control.input(Key::None, 'r'), EventProcessStatus::Ignored));
                self.observed.store(true, Ordering::Relaxed);
            }
        }
    }
    #[test]
    fn outer_terminal_shrink_pauses_active_snake_and_disables_clipped_games() {
        let _ui_lock = UI_LOCK.lock().unwrap();
        for kind in GameKind::ALL {
            for (width, height, script) in [
                (80, 24, "Paint.Enable(false)\nResize(30,12)\nResize(80,24)"),
                (30, 12, "Paint.Enable(false)\nResize(80,24)"),
            ] {
                let observed = Arc::new(AtomicBool::new(false));
                let desktop = ResizeProbe { base: Desktop::new(), window: Handle::None, kind, observed: observed.clone() };
                App::debug(width, height, script).desktop(desktop).timers_count(4).build().unwrap().run();
                assert!(observed.load(Ordering::Relaxed));
            }
        }
    }
    #[Window(events = WindowEvents)]
    struct InputProbe { game: Handle<GameControl>, kind: GameKind, observed: Arc<AtomicBool> }
    impl WindowEvents for InputProbe {
        fn on_cancel(&mut self) -> ActionRequest {
            let control = self.control(self.game).unwrap();
            assert!(control.origin(control.viewport).is_some(), "layout must reach the game control");
            match self.kind {
                GameKind::Snake => assert_eq!(control.game.phase(), Phase::Paused),
                GameKind::Puzzle2048 => assert_eq!(control.game.score(), 4),
                GameKind::Minesweeper => if let Game::Minesweeper(game) = &control.game {
                    assert!(game.cells[40].revealed && !game.cells[40].mine);
                },
            }
            self.observed.store(true, Ordering::Relaxed);
            ActionRequest::Allow
        }
    }
    #[test]
    fn actual_appcui_layout_and_focus_deliver_keys_after_resize() {
        let _ui_lock = UI_LOCK.lock().unwrap();
        for (kind, keys) in [
            (GameKind::Snake, "Key.Pressed(Right)\nKey.Pressed(P)\n"),
            (GameKind::Puzzle2048, "Key.Pressed(Left)\n"),
            (GameKind::Minesweeper, "Key.Pressed(Space)\n"),
        ] {
            let script = format!("Paint.Enable(false)\n{keys}Resize(30,12)\nResize(80,24)\nKey.Pressed(Escape)");
            let mut app = App::debug(80, 24, &script).build().unwrap();
            let observed = Arc::new(AtomicBool::new(false));
            let mut window = InputProbe { base: Window::new(kind.name(), layout!("a:c,w:66,h:22"), window::Flags::Sizeable), game: Handle::None, kind, observed: observed.clone() };
            let mut control = GameControl::new(kind, 1);
            if let Game::Puzzle2048(game) = &mut control.game { game.board = [2,2,0,0, 0,0,0,0, 0,0,0,0, 0,0,0,0]; }
            window.game = window.add(control);
            window.base.request_focus_for_control(window.game);
            app.add_window(window);
            app.run();
            assert!(observed.load(Ordering::Relaxed), "{kind:?}: assertions must run before closing");
        }
    }
    #[test]
    fn hidden_or_unfocused_snake_timer_pauses_and_close_is_idempotent() {
        let _ui_lock = UI_LOCK.lock().unwrap();
        let app = App::debug(80, 24, "Paint.Enable(false)").timers_count(4).build().unwrap();
        let mut window = GameWindow::new(GameKind::Snake).unwrap();
        let handle = window.game;
        for visible in [true, false] {
            let control = window.control_mut(handle).unwrap();
            if let Game::Snake(game) = &mut control.game { game.phase = Phase::Playing; }
            window.set_visible(visible);
            assert!(matches!(TimerEvents::on_update(&mut window, 0), EventProcessStatus::Ignored));
            assert_eq!(window.control(handle).unwrap().game.phase(), Phase::Paused);
        }
        assert!(matches!(WindowEvents::on_cancel(&mut window), ActionRequest::Allow));
        assert!(window.is_closing());
        window.shutdown(); window.shutdown();
        app.run();
    }
    #[test]
    fn snake_controls_pause_restart_and_preserve_session_best() {
        let _ui_lock = UI_LOCK.lock().unwrap();
        let app = App::debug(80, 24, "Paint.Enable(false)").build().unwrap();
        let mut control = GameControl::new(GameKind::Snake, 1);
        control.set_size(48, 20);
        OnResize::on_resize(&mut control, Size::new(0, 0), Size::new(48, 20));
        if let Game::Snake(game) = &mut control.game { game.food = Some((7, 6)); }
        assert!(matches!(control.input(Key::None, 'd'), EventProcessStatus::Processed));
        assert!(control.advance());
        assert_eq!(control.game.score(), 10);
        OnFocus::on_lose_focus(&mut control);
        assert_eq!(control.game.phase(), Phase::Paused);
        assert!(!control.advance());
        control.input(key!("Space").into(), ' ');
        assert_eq!(control.game.phase(), Phase::Playing);
        control.input(Key::None, 'r');
        assert_eq!(control.game.phase(), Phase::Ready);
        assert_eq!(control.best, 10);
        assert_eq!(control.game.score(), 0);
        assert!(matches!(control.input(key!("Ctrl+R").into(), 'r'), EventProcessStatus::Ignored));
        assert!(matches!(OnKeyPressed::on_key_pressed(&mut control, key!("Right").into(), '\0'), EventProcessStatus::Ignored));
        app.run();
    }
    #[test]
    fn shrinking_pauses_and_small_windows_ignore_board_input() {
        let _ui_lock = UI_LOCK.lock().unwrap();
        let app = App::debug(80, 24, "Paint.Enable(false)").build().unwrap();
        let mut control = GameControl::new(GameKind::Snake, 1);
        control.set_size(48, 20);
        OnResize::on_resize(&mut control, Size::new(0, 0), Size::new(48, 20));
        control.input(Key::None, 'd');
        control.set_size(20, 8);
        OnResize::on_resize(&mut control, Size::new(48, 20), Size::new(20, 8));
        assert_eq!(control.game.phase(), Phase::Paused);
        assert!(!control.advance());
        control.input(Key::None, 'd'); control.input(Key::None, 'r');
        assert_eq!(control.game.phase(), Phase::Paused);
        control.set_size(48, 20);
        OnResize::on_resize(&mut control, Size::new(20, 8), Size::new(48, 20));
        assert_eq!(control.game.phase(), Phase::Paused);
        control.input(Key::None, 'p');
        assert_eq!(control.game.phase(), Phase::Playing);
        app.run();
    }
    #[test]
    fn minesweeper_mouse_and_keyboard_address_the_same_cell() {
        let _ui_lock = UI_LOCK.lock().unwrap();
        let app = App::debug(80, 24, "Paint.Enable(false)").build().unwrap();
        let mut control = GameControl::new(GameKind::Minesweeper, 1);
        control.set_size(50, 22);
        OnResize::on_resize(&mut control, Size::new(0, 0), Size::new(50, 22));
        let click = |button| MouseEvent::Pressed(MouseEventData { x: 24, y: 8, button, modifier: KeyModifier::None });
        OnMouseEvent::on_mouse_event(&mut control, &click(MouseButton::Right));
        if let Game::Minesweeper(game) = &control.game { assert!(game.cells[40].flagged); }
        OnMouseEvent::on_mouse_event(&mut control, &click(MouseButton::Left));
        assert_eq!(control.game.phase(), Phase::Ready);
        control.input(Key::None, 'f');
        control.input(key!("Space").into(), ' ');
        if let Game::Minesweeper(game) = &control.game { assert!(game.cells[40].revealed && !game.cells[40].mine); }
        control.input(key!("Right").into(), '\0');
        assert_eq!(control.cursor, 41);
        assert!(matches!(OnMouseEvent::on_mouse_event(&mut control, &MouseEvent::Pressed(MouseEventData {
            x: -1, y: 4, button: MouseButton::Left, modifier: KeyModifier::None,
        })), EventProcessStatus::Ignored));
        app.run();
    }
    #[test]
    fn boards_and_resize_prompt_render_with_light_and_dark_theme_colors() {
        let _ui_lock = UI_LOCK.lock().unwrap();
        let app = App::debug(80, 24, "Paint.Enable(false)").build().unwrap();
        for kind in GameKind::ALL {
            let mut control = GameControl::new(kind, 1);
            for theme in [Theme::new(Themes::Light), Theme::new(Themes::DarkGray)] {
                for (width, height) in [(1, 1), (20, 8), (48, 20), (80, 24)] {
                    let mut surface = Surface::new(width, height);
                    OnResize::on_resize(&mut control, Size::new(0, 0), Size::new(width, height));
                    OnPaint::on_paint(&control, &mut surface, &theme);
                    if let Some((x, y)) = control.origin(surface.size()) {
                        assert_eq!(surface.char(x, y).unwrap().code, '┌', "{kind:?}");
                        assert_eq!(surface.char(x, y).unwrap().foreground, theme.lines.normal.foreground);
                    } else {
                        assert_eq!(surface.char(0, 0).unwrap().code, 'R');
                        assert_eq!(surface.char(0, 0).unwrap().foreground, theme.text.normal.foreground);
                    }
                }
            }
        }
        app.run();
    }
    #[test]
    fn board_paint_and_mouse_share_the_window_viewport_on_a_terminal_surface() {
        let _ui_lock = UI_LOCK.lock().unwrap();
        let app = App::debug(80, 24, "Paint.Enable(false)").build().unwrap();
        let mut control = GameControl::new(GameKind::Minesweeper, 1);
        OnResize::on_resize(&mut control, Size::new(0, 0), Size::new(64, 20));
        // AppCUI keeps the full terminal surface, translating and clipping each
        // control's paint rather than allocating a surface of the control size.
        let mut surface = Surface::new(80, 24);
        surface.set_origin(8, 2);
        surface.set_clip(8, 2, 71, 21);
        OnPaint::on_paint(&control, &mut surface, &Theme::new(Themes::Light));
        assert_eq!(surface.char(17, 3).unwrap().code, '┌');
        assert_eq!(surface.char(19, 4).unwrap().code, '#');
        OnMouseEvent::on_mouse_event(&mut control, &MouseEvent::Pressed(MouseEventData {
            x: 19, y: 4, button: MouseButton::Right, modifier: KeyModifier::None,
        }));
        OnPaint::on_paint(&control, &mut surface, &Theme::new(Themes::Light));
        assert_eq!(surface.char(19, 4).unwrap().code, 'F');
        OnResize::on_resize(&mut control, Size::new(64, 20), Size::new(20, 8));
        OnPaint::on_paint(&control, &mut surface, &Theme::new(Themes::Light));
        assert_eq!(surface.char(0, 0).unwrap().code, 'R');
        app.run();
    }
}
