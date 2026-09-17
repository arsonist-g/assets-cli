slint::include_modules!();

mod app;

fn main() -> Result<(), slint::PlatformError> {
    app::run()
}
