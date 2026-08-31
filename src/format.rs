use crate::palette::{ansi_truecolor, Rgb};

pub fn render_ansi(template: &str, colors: &[Rgb]) -> Result<String, String> {
    let mut rendered = template.to_owned();
    let mut used = false;
    for (index, color) in colors.iter().enumerate().rev() {
        let placeholder = format!("${}", index + 1);
        if rendered.contains(&placeholder) {
            used = true;
            rendered = rendered.replace(&placeholder, &format!("\x1b[{}m", ansi_truecolor(*color)));
        }
    }
    if !used {
        return Err("template contains none of the available $1, $2, ... placeholders".to_owned());
    }
    if rendered.ends_with('\n') {
        rendered.pop();
        rendered.push_str("\x1b[0m\n");
    } else {
        rendered.push_str("\x1b[0m");
    }
    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_placeholders() {
        let rendered = render_ansi("$1red $2blue\n", &[[255, 0, 0], [0, 0, 255]]).unwrap();
        assert_eq!(
            rendered,
            "\x1b[38;2;255;0;0mred \x1b[38;2;0;0;255mblue\x1b[0m\n"
        );
    }

    #[test]
    fn rejects_template_without_placeholders() {
        assert!(render_ansi("plain text", &[[255, 0, 0]]).is_err());
    }
}
