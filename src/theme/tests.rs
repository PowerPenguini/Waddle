use super::*;

#[test]
fn parses_gtk_selection_colors() {
    let colors = parse_theme_css(
        "@define-color theme_selected_bg_color #2eb398;\n\
         @define-color theme_selected_fg_color #fff;",
    )
    .unwrap();

    assert_eq!(colors.accent, Color::from_rgb8(46, 179, 152));
    assert_eq!(colors.selection_foreground, Some(Color::WHITE));
}

#[test]
fn resolves_symbolic_gtk_colors() {
    let colors = parse_theme_css(
        "@define-color selected_bg_color @accent_color;\n\
         @define-color accent_color #369;\n\
         @define-color selected_fg_color black;",
    )
    .unwrap();

    assert_eq!(colors.accent, Color::from_rgb8(51, 102, 153));
    assert_eq!(colors.selection_foreground, Some(Color::BLACK));
}
