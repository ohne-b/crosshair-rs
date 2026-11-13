#[path = "../../packaging/windows/icon.rs"]
mod icon;

fn main() {
    icon::embed();
    slint_build::compile_with_config(
        "ui/app.slint",
        slint_build::CompilerConfiguration::new().with_style("fluent-dark".into()),
    )
    .expect("Slint interface must compile");
}
