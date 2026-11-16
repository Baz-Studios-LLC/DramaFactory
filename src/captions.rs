//! Line-timed captions. Long lines are divided for readability; their internal
//! chunk times are estimated by word length, not claimed as forced alignment.
use serde::{Deserialize, Serialize};
#[derive(Clone, Serialize, Deserialize)]
pub struct Caption {
    pub start: f64,
    pub end: f64,
    pub text: String,
}
fn stamp(seconds: f64, ass: bool) -> String {
    let scale = if ass { 100. } else { 1000. };
    let ticks = (seconds.max(0.) * scale).round() as u64;
    let seconds = ticks / scale as u64;
    if ass {
        format!(
            "{}:{:02}:{:02}.{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60,
            ticks % 100
        )
    } else {
        format!(
            "{:02}:{:02}:{:02},{:03}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60,
            ticks % 1000
        )
    }
}
fn readable(captions: &[Caption]) -> Vec<Caption> {
    let mut output = Vec::new();
    for line in captions {
        let mut chunks = Vec::<String>::new();
        let mut current = String::new();
        for word in line.text.split_whitespace() {
            if !current.is_empty() && current.chars().count() + word.chars().count() + 1 > 68 {
                chunks.push(std::mem::take(&mut current));
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        }
        if !current.is_empty() {
            chunks.push(current);
        }
        let total: usize = chunks.iter().map(|c| c.chars().count()).sum();
        let mut position = 0;
        for text in chunks {
            let start =
                line.start + (line.end - line.start) * position as f64 / total.max(1) as f64;
            position += text.chars().count();
            let end = line.start + (line.end - line.start) * position as f64 / total.max(1) as f64;
            output.push(Caption { start, end, text });
        }
    }
    output.sort_by(|a, b| a.start.total_cmp(&b.start));
    output
}
pub fn srt(captions: &[Caption]) -> String {
    readable(captions)
        .iter()
        .enumerate()
        .map(|(i, c)| {
            format!(
                "{}\n{} --> {}\n{}\n\n",
                i + 1,
                stamp(c.start, false),
                stamp(c.end, false),
                c.text
            )
        })
        .collect()
}
pub fn ass(captions: &[Caption], width: u32, height: u32) -> String {
    let mut out = format!(
        "[Script Info]\nScriptType: v4.00+\nPlayResX: {width}\nPlayResY: {height}\nWrapStyle: 0\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Dialogue,Arial,56,&H00FFFFFF,&H00FFFFFF,&H20000000,&H80000000,0,0,0,0,100,100,0,0,1,3,1,2,90,90,150,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n"
    );
    for c in readable(captions) {
        // Renderer controls are represented as visible Unicode alternatives;
        // never interpret dialogue as ASS override commands.
        let text = c
            .text
            .replace('\\', "＼")
            .replace('{', "｛")
            .replace('}', "｝");
        out.push_str(&format!(
            "Dialogue: 0,{},{},Dialogue,,0,0,0,,{}\n",
            stamp(c.start, true),
            stamp(c.end, true),
            text
        ));
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn caption_clock_carries_and_long_lines_keep_exact_outer_bounds() {
        assert_eq!(stamp(59.9996, false), "00:01:00,000");
        let lines=[Caption{start:1.25,end:5.75,text:"One deliberately long line with enough words to require two readable caption chunks without losing the final endpoint.".into()}];
        let readable = readable(&lines);
        assert!(readable.len() > 1);
        assert_eq!(readable[0].start, 1.25);
        assert_eq!(readable.last().unwrap().end, 5.75);
        assert_eq!(readable[0].end, readable[1].start);
        assert!(srt(&lines).contains("00:00:01,250"));
        assert!(
            !ass(
                &[Caption {
                    start: 0.,
                    end: 1.,
                    text: r"{\pos(0,0)}Hello".into()
                }],
                1080,
                1920
            )
            .contains(r"{\pos")
        );
    }
}
