use rune::text::Span;
use rune::tok::{self, Family, TextStyle};
use rune::{AppSpec, Color, Cx, El, col, rich};

struct S;
fn view(_: &S, _: &mut Cx<S>) -> El {
    let t = tok::theme(true);
    let line = "user@host ~/src % ls -la";
    let spans = vec![
        Span::new(0, 9).color(Color::WHITE),
        Span::new(10, 16).color(tok::accent(1, true)),
    ];
    col().w_full().h_full().padding(10.0).fill(t.bg).child(
        rich(line, spans)
            .font(TextStyle::new(13.0, 400.0).family(Family::SystemMono))
            .color(t.text),
    )
}
fn main() {
    rune::run(AppSpec::new("T", (400.0, 120.0), || S, view));
}
