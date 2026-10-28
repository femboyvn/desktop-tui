use appcui::prelude::*;

#[derive(Debug)]
struct HelpTopic {
    title: String,
    body: String,
}
fn topics(source: &str) -> Vec<HelpTopic> {
    let mut result = Vec::new();
    let mut title = "Overview".to_owned();
    let mut body = String::new();
    for line in source.lines() {
        if line.starts_with("# ") {
            continue;
        }
        if let Some(heading) = line.strip_prefix("## ") {
            result.push(HelpTopic {
                title,
                body: body.trim().to_owned(),
            });
            body = String::new();
            title = match heading {
                "Launching applications" => "Launching apps",
                "Preferences and Femboy palette" => "Preferences",
                "Shutdown and Restart" => "Power actions",
                other => other,
            }
            .to_owned();
        } else {
            body.push_str(line);
            body.push('\n');
        }
    }
    result.push(HelpTopic {
        title,
        body: body.trim().to_owned(),
    });
    result
}
fn reflow(source: &str, width: usize) -> String {
    let width = width.max(1);
    let mut result = Vec::new();
    let mut paragraph = String::new();
    let flush = |paragraph: &mut String, result: &mut Vec<String>| {
        let mut words = Vec::new();
        let mut word = String::new();
        let mut in_code = false;
        for ch in paragraph.chars() {
            if ch == '`' {
                in_code = !in_code;
            }
            if ch.is_whitespace() && !in_code {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            } else {
                word.push(ch);
            }
        }
        if !word.is_empty() {
            words.push(word);
        }
        let mut line = String::new();
        let mut count = 0;
        for word in words {
            let length = word.chars().count();
            if count > 0 && count + 1 + length > width {
                result.push(std::mem::take(&mut line));
                count = 0;
            }
            if count > 0 {
                line.push(' ');
                count += 1;
            }
            line.push_str(&word);
            count += length;
        }
        if !line.is_empty() {
            result.push(line);
        }
        paragraph.clear();
    };
    for line in source.lines() {
        if line.is_empty() || line.starts_with('#') {
            flush(&mut paragraph, &mut result);
            result.push(line.to_owned());
        } else {
            if !paragraph.is_empty() {
                paragraph.push(' ');
            }
            paragraph.push_str(line.trim());
        }
    }
    flush(&mut paragraph, &mut result);
    result.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn help_opens_scrolls_and_closes_at_80x24() {
        let _ui_lock = crate::test_support::UI_LOCK.lock().unwrap();
        let mut app = App::debug(80, 24, "Key.Pressed(PageDown)\nKey.Pressed(Escape)\nPaint('end')")
            .build()
            .unwrap();
        app.add_window(help_window());
        app.run();
    }
    #[test]
    fn bundled_help_is_split_into_named_categories_with_overview() {
        let topics = topics(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/HELP.md")));
        assert_eq!(topics[0].title, "Overview");
        assert!(topics[0].body.contains("offline"));
        assert!(
            topics
                .iter()
                .any(|topic| topic.title == "Launching apps" && topic.body.contains("Start"))
        );
        assert!(
            topics
                .iter()
                .any(|topic| topic.title == "Preferences" && topic.body.contains("#RRGGBB"))
        );
        assert!(
            topics
                .iter()
                .any(|topic| topic.title == "Power actions" && topic.body.contains("inhibitor"))
        );
        assert!(!topics[0].body.contains("Shutdown"));
    }
    #[test]
    fn category_selection_updates_details_and_details_scroll_after_resize() {
        let _ui_lock = crate::test_support::UI_LOCK.lock().unwrap();
        let mut app=App::debug(80,24,"Key.Pressed(Down)\nKey.Pressed(Tab)\nKey.Pressed(PageDown)\nResize(70,20)\nKey.Pressed(Escape)\nPaint('end')").build().unwrap();
        let mut help = help_window();
        let original_count = help.topics.len();
        let category = help.categories;
        assert!(
            help.control(help.details)
                .unwrap()
                .source
                .contains("offline")
        );
        let window_controls = help.topics.iter().position(|topic| topic.title == "Window controls").unwrap();
        ListBoxEvents::on_current_item_changed(&mut help, category, window_controls);
        let details_handle = help.details;
        let details = help.control_mut(details_handle).unwrap();
        assert!(details.source.contains("Drag the title bar"));
        assert!(!details.source.contains("inhibitor"));
        details.set_size(34, 8);
        OnResize::on_resize(details, Size::new(0, 0), Size::new(34, 8));
        let narrow_count = details.lines.len();
        OnKeyPressed::on_key_pressed(details, key!("PageDown").into(), '\0');
        assert!(details.offset > 0);
        OnKeyPressed::on_key_pressed(details, key!("Home").into(), '\0');
        assert_eq!(details.offset, 0);
        OnResize::on_resize(details, Size::new(34, 8), Size::new(60, 12));
        assert!(details.lines.len() < narrow_count);
        assert_eq!(help.topics.len(), original_count);
        app.add_window(help);
        app.run();
    }
    #[test]
    fn detail_reflows_paragraphs_without_breaking_markdown_code() {
        let source = "# Screen Lock\n\nLock using `loginctl lock-session` with `XDG_SESSION_ID` in your local graphical desktop session.\nThe desktop authenticates the session.\n\nChoose Screen Lock.\n";
        let narrow = reflow(source, 36);
        assert!(narrow.contains("`loginctl lock-session`"));
        assert!(narrow.contains("`XDG_SESSION_ID`"));
        assert!(narrow.lines().all(|line| line.chars().count() <= 36));
        assert!(
            narrow
                .lines()
                .all(|line| line.chars().filter(|c| *c == '`').count() % 2 == 0)
        );
        let wide = reflow(source, 70);
        assert!(wide.lines().count() < narrow.lines().count());
        assert!(narrow.contains("\n\nChoose Screen Lock."));
    }
}

#[CustomControl(overwrite = OnResize+OnPaint+OnKeyPressed+OnMouseEvent)]
struct HelpDetails {
    source: String,
    lines: Vec<String>,
    offset: usize,
}
impl HelpDetails {
    fn new() -> Self {
        Self {
            base: ControlBase::new(layout!("d:f"), true),
            source: String::new(),
            lines: Vec::new(),
            offset: 0,
        }
    }
    fn set_topic(&mut self, topic: &HelpTopic) {
        self.source = format!("# {}\n\n{}", topic.title, topic.body);
        self.offset = 0;
        self.render(self.size());
    }
    fn render(&mut self, size: Size) {
        let width = size.width.saturating_sub(2).max(1) as usize;
        self.lines = reflow(&self.source, width)
            .lines()
            .flat_map(|line| {
                let plain: Vec<char> = line
                    .strip_prefix("# ")
                    .unwrap_or(line)
                    .replace('`', "")
                    .chars()
                    .collect();
                if plain.is_empty() {
                    vec![String::new()]
                } else {
                    plain
                        .chunks(width)
                        .map(|chunk| chunk.iter().collect())
                        .collect()
                }
            })
            .collect();
        self.offset = self
            .offset
            .min(self.lines.len().saturating_sub(size.height as usize));
    }
    fn scroll(&mut self, delta: i32) {
        self.offset = self
            .offset
            .saturating_add_signed(delta as isize)
            .min(self.lines.len().saturating_sub(self.size().height as usize));
    }
}
impl OnResize for HelpDetails {
    fn on_resize(&mut self, _old_size: Size, new_size: Size) {
        self.render(new_size);
    }
}
impl OnPaint for HelpDetails {
    fn on_paint(&self, surface: &mut Surface, theme: &Theme) {
        surface.clear(Character::with_attributes(' ', theme.window.normal));
        let size = surface.size();
        for (row, line) in self
            .lines
            .iter()
            .skip(self.offset)
            .take(size.height as usize)
            .enumerate()
        {
            let mut attr = theme.text.normal;
            if row == 0 && self.offset == 0 {
                attr.flags |= CharFlags::Bold;
            }
            surface.write_string(0, row as i32, line, attr, false);
        }
        if self.lines.len() > size.height as usize && size.height > 1 {
            let x = size.width as i32 - 1;
            surface.fill_vertical_line(
                x,
                0,
                size.height as i32 - 1,
                Character::with_attributes('│', theme.scrollbar.bar.normal),
            );
            let max = self.lines.len() - size.height as usize;
            let position = self.offset * (size.height as usize - 1) / max;
            surface.write_char(
                x,
                position as i32,
                Character::with_attributes('█', theme.scrollbar.position.focused),
            );
        }
    }
}
impl OnKeyPressed for HelpDetails {
    fn on_key_pressed(&mut self, key: Key, _character: char) -> EventProcessStatus {
        match key.value() {
            key!("Up") => self.scroll(-1),
            key!("Down") => self.scroll(1),
            key!("PageUp") => self.scroll(-(self.size().height.max(1) as i32)),
            key!("PageDown") => self.scroll(self.size().height.max(1) as i32),
            key!("Home") => self.offset = 0,
            key!("End") => {
                self.offset = self.lines.len().saturating_sub(self.size().height as usize)
            }
            _ => return EventProcessStatus::Ignored,
        }
        EventProcessStatus::Processed
    }
}
impl OnMouseEvent for HelpDetails {
    fn on_mouse_event(&mut self, event: &MouseEvent) -> EventProcessStatus {
        match event {
            MouseEvent::Wheel(MouseWheelDirection::Up) => self.scroll(-3),
            MouseEvent::Wheel(MouseWheelDirection::Down) => self.scroll(3),
            MouseEvent::Pressed(data)
                if data.x == self.size().width as i32 - 1 && self.size().height > 1 =>
            {
                let max = self.lines.len().saturating_sub(self.size().height as usize);
                self.offset =
                    (data.y.max(0) as usize * max / (self.size().height as usize - 1)).min(max);
            }
            _ => return EventProcessStatus::Ignored,
        }
        EventProcessStatus::Processed
    }
}

#[Window(events = ListBoxEvents)]
pub struct HelpWindow {
    categories: Handle<ListBox>,
    details: Handle<HelpDetails>,
    topics: Vec<HelpTopic>,
}
impl HelpWindow {
    pub fn focus_categories(&mut self) {
        self.base.request_focus_for_control(self.categories);
    }
    fn new() -> Self {
        let topics = topics(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/HELP.md")));
        let mut window = Self {
            base: Window::new("Help", layout!("a:c,w:76,h:22"), window::Flags::Sizeable),
            categories: Handle::None,
            details: Handle::None,
            topics,
        };
        window.set_size_bounds(54, 12, u16::MAX, u16::MAX);
        let mut split = VSplitter::new(
            22,
            layout!("l:0,t:0,r:0,b:2"),
            vsplitter::ResizeBehavior::PreserveLeftPanelSize,
        );
        split.set_min_width(vsplitter::Panel::Left, 18);
        split.set_min_width(vsplitter::Panel::Right, 24);
        split.add(
            vsplitter::Panel::Left,
            Label::new("Categories", layout!("l:1,t:0,r:1,h:1")),
        );
        let mut categories = ListBox::new(
            layout!("l:0,t:1,r:0,b:0"),
            listbox::Flags::ScrollBars | listbox::Flags::HighlightSelectedItemWhenInactive,
        );
        for topic in &window.topics {
            categories.add(&topic.title);
        }
        categories.set_index(0);
        window.categories = split.add(vsplitter::Panel::Left, categories);
        let mut details = HelpDetails::new();
        details.set_topic(&window.topics[0]);
        window.details = split.add(vsplitter::Panel::Right, details);
        window.add(split);
        window.add(Label::new(
            "Up/Down: category | Tab: details | PgUp/PgDn: scroll",
            layout!("l:1,b:0,r:1,h:1"),
        ));
        window.base.request_focus_for_control(window.categories);
        window
    }
}
impl ListBoxEvents for HelpWindow {
    fn on_current_item_changed(
        &mut self,
        handle: Handle<ListBox>,
        index: usize,
    ) -> EventProcessStatus {
        if handle == self.categories {
            if let Some(topic) = self.topics.get(index) {
                if let Some(details) = self.base.control_mut(self.details) {
                    details.set_topic(topic);
                }
            }
            EventProcessStatus::Processed
        } else {
            EventProcessStatus::Ignored
        }
    }
}
pub fn help_window() -> HelpWindow {
    HelpWindow::new()
}
