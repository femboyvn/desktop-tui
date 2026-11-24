mod image_viewer;
mod browser;
mod music_player;
mod games;
mod pdf_reader;

pub(crate) use image_viewer::ImageWindow;
pub(crate) use browser::{BrowserWindow, BrowserAction};
pub(crate) use music_player::MusicWindow;
pub(crate) use games::GameWindow;
pub(crate) use pdf_reader::PdfWindow;
pub(crate) use image_viewer::select_image;

mod ascii_paint;
pub(crate) use ascii_paint::PaintWindow;
pub(crate) mod kanban;
pub(crate) use kanban::{KanbanWindow,KanbanAction};

mod ai_chat;
pub(crate) use ai_chat::AiChatWindow;

mod ai_settings;
