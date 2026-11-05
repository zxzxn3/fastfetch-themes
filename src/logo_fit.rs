use std::fs;
use crate::json::{parse_json, parse_jsonc};
use crate::sys::tty_winsize;

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
pub fn run(args: &[String]) -> Result<(), String> {
    let Some(path) = args.first() else {
        return Err(String::from("usage: logo-fit <config.jsonc>"));
    };
    let text = fs::read_to_string(path).map_err(|e| format!("failed to read {path}: {e}"))?;
    let config = parse_jsonc(&text).ok_or_else(|| format!("invalid JSONC in {path}"))?;

    let logo = config.get("logo");
    let mut width = logo.get("width").i64_or(40);
    let padding_top = logo.get("padding").get("top").i64_or(0);
    let logo_pos = logo.get("position").str_or("auto");

    if logo_pos == "top" {
        println!("--logo-width\n{width}\n--logo-padding-top\n{padding_top}\n--logo-position\ntop");
        return Ok(());
    }

    let first_line = text.lines().next().unwrap_or("");
    let (height, content) = if let Some(rest) = first_line.strip_prefix("// layout:") {
        let meta = parse_json(rest.trim()).ok_or_else(|| String::from("invalid // layout JSON"))?;
        (meta.get("rows").i64_or(24), meta.get("columns").i64_or(60))
    } else {
        let mod_len = config.get("modules").as_arr().map_or(0, <[crate::json::JsonVal]>::len);
        (i64::try_from(mod_len).unwrap_or(24), 60)
    };

    let mut ratio = 2.0_f64;
    let cols = if let Some(ws) = tty_winsize() {
        if ws.cols > 0 && ws.rows > 0 && ws.xpixel > 0 && ws.ypixel > 0 {
            ratio = (f64::from(ws.ypixel) / f64::from(ws.rows))
                / (f64::from(ws.xpixel) / f64::from(ws.cols));
        }
        i64::from(ws.cols)
    } else {
        0
    };

    let (top, position) = if cols > 0 {
        let available = cols - content - 6;
        if available >= 22 {
            let calc_w = ((height - 2) as f64 * ratio).round_ties_even() as i64;
            width = calc_w.min(available);
            let calc_top = ((height as f64 - (width as f64 / ratio)) / 2.0).round_ties_even() as i64;
            (calc_top.max(0), "left")
        } else {
            width = width.min((cols - 4).max(16)).min(40);
            (0, "top")
        }
    } else {
        (padding_top, logo_pos)
    };

    println!("--logo-width\n{width}\n--logo-padding-top\n{top}\n--logo-position\n{position}");
    Ok(())
}

