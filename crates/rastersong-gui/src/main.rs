use iced::{Element, Task};

fn main() -> iced::Result {
    // Initialize the core library
    rastersong::init();

    iced::application("RasterSong", update, view).run()
}

#[derive(Debug, Clone)]
enum Message {}

fn update<'a>(_value: &mut (), _message: Message) -> Task<Message> {
    Task::none()
}

fn view(_value: &()) -> Element<'_, Message> {
    iced::widget::text("RasterSong").into()
}
