use super::*;

#[test]
fn mmdc_render_args_include_configured_background_color() {
    let args = mmdc_render_args(
        Path::new("/tmp/diagram.mmd"),
        Path::new("/tmp/diagram.png"),
        2,
        "#ffffff",
    )
    .into_iter()
    .map(|arg| arg.to_string_lossy().into_owned())
    .collect::<Vec<_>>();

    assert_eq!(
        args,
        vec![
            "-i",
            "/tmp/diagram.mmd",
            "-o",
            "/tmp/diagram.png",
            "-b",
            "#ffffff",
            "--scale",
            "2",
        ]
    );
}
