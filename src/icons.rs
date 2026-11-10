//! Small vector icons drawn with the painter — crisp at any DPI, and no
//! icon font or image assets to ship.

use std::f32::consts::{PI, TAU};

use eframe::egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2, pos2, vec2};

fn stroke(c: Color32, w: f32) -> Stroke {
    Stroke::new(w, c)
}

pub fn play(p: &Painter, c: Pos2, s: f32, col: Color32) {
    let h = s * 0.5;
    p.add(Shape::convex_polygon(
        vec![pos2(c.x - h * 0.7, c.y - h), pos2(c.x + h * 0.95, c.y), pos2(c.x - h * 0.7, c.y + h)],
        col,
        Stroke::NONE,
    ));
}

pub fn pause(p: &Painter, c: Pos2, s: f32, col: Color32) {
    let (w, h) = (s * 0.22, s * 0.5);
    p.rect_filled(Rect::from_min_max(pos2(c.x - w * 1.8, c.y - h), pos2(c.x - w * 0.6, c.y + h)), 1.0, col);
    p.rect_filled(Rect::from_min_max(pos2(c.x + w * 0.6, c.y - h), pos2(c.x + w * 1.8, c.y + h)), 1.0, col);
}

/// dir = -1 for previous, +1 for next.
pub fn skip(p: &Painter, c: Pos2, s: f32, col: Color32, dir: f32) {
    let h = s * 0.42;
    let tip = c.x + dir * h * 0.55;
    let back = c.x - dir * h * 0.75;
    p.add(Shape::convex_polygon(
        vec![pos2(back, c.y - h), pos2(tip, c.y), pos2(back, c.y + h)],
        col,
        Stroke::NONE,
    ));
    let bar_x = tip + dir * s * 0.1;
    p.line_segment([pos2(bar_x, c.y - h), pos2(bar_x, c.y + h)], stroke(col, s * 0.13));
}

pub fn chevron(p: &Painter, c: Pos2, s: f32, col: Color32, open: bool) {
    let h = s * 0.28;
    let pts = if open {
        [pos2(c.x - h, c.y - h * 0.5), pos2(c.x, c.y + h * 0.5), pos2(c.x + h, c.y - h * 0.5)]
    } else {
        [pos2(c.x - h * 0.5, c.y - h), pos2(c.x + h * 0.5, c.y), pos2(c.x - h * 0.5, c.y + h)]
    };
    p.line_segment([pts[0], pts[1]], stroke(col, 1.3));
    p.line_segment([pts[1], pts[2]], stroke(col, 1.3));
}

pub fn heart(p: &Painter, c: Pos2, s: f32, col: Color32, filled: bool) {
    // Classic parametric heart, scaled into an s×s box.
    let n = 48;
    let k = s / 34.0;
    let pts: Vec<Pos2> = (0..n)
        .map(|i| {
            let t = i as f32 / n as f32 * TAU;
            let x = 16.0 * t.sin().powi(3);
            let y = 13.0 * t.cos() - 5.0 * (2.0 * t).cos() - 2.0 * (3.0 * t).cos() - (4.0 * t).cos();
            pos2(c.x + x * k, c.y - y * k + s * 0.04)
        })
        .collect();
    if filled {
        // Not convex, so fill as a fan of triangles from an interior point.
        let center = pos2(c.x, c.y + s * 0.05);
        let mut mesh = eframe::egui::Mesh::default();
        mesh.colored_vertex(center, col);
        for pt in &pts {
            mesh.colored_vertex(*pt, col);
        }
        for i in 0..n as u32 {
            mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n as u32);
        }
        p.add(Shape::mesh(mesh));
    }
    p.add(Shape::closed_line(pts, stroke(col, 1.3)));
}

pub fn shuffle(p: &Painter, c: Pos2, s: f32, col: Color32) {
    let (w, h) = (s * 0.45, s * 0.28);
    let st = stroke(col, 1.4);
    // Two crossing paths with arrowheads on the right.
    p.line_segment([pos2(c.x - w, c.y - h), pos2(c.x - w * 0.35, c.y - h)], st);
    p.line_segment([pos2(c.x - w * 0.35, c.y - h), pos2(c.x + w * 0.35, c.y + h)], st);
    p.line_segment([pos2(c.x + w * 0.35, c.y + h), pos2(c.x + w, c.y + h)], st);
    p.line_segment([pos2(c.x - w, c.y + h), pos2(c.x - w * 0.35, c.y + h)], st);
    p.line_segment([pos2(c.x - w * 0.35, c.y + h), pos2(c.x + w * 0.35, c.y - h)], st);
    p.line_segment([pos2(c.x + w * 0.35, c.y - h), pos2(c.x + w, c.y - h)], st);
    for y in [c.y - h, c.y + h] {
        let a = s * 0.16;
        p.line_segment([pos2(c.x + w - a, y - a), pos2(c.x + w, y)], st);
        p.line_segment([pos2(c.x + w - a, y + a), pos2(c.x + w, y)], st);
    }
}

pub fn lock(p: &Painter, c: Pos2, s: f32, col: Color32, closed: bool) {
    let body = Rect::from_center_size(pos2(c.x, c.y + s * 0.16), vec2(s * 0.62, s * 0.46));
    p.rect_filled(body, 2.0, col);
    let r = s * 0.2;
    let arc_c = pos2(c.x, body.top() - s * 0.08);
    // Shackle: a half-circle; when open it's lifted and shifted right.
    let (cx, lift) = if closed { (arc_c.x, 0.0) } else { (arc_c.x + r * 0.9, s * 0.08) };
    let pts: Vec<Pos2> = (0..=16)
        .map(|i| {
            let a = PI + PI * i as f32 / 16.0;
            pos2(cx + r * a.cos(), arc_c.y - lift + r * a.sin())
        })
        .collect();
    let st = stroke(col, s * 0.11);
    p.add(Shape::line(pts, st));
    p.line_segment([pos2(cx - r, arc_c.y - lift), pos2(cx - r, body.top())], st);
    if closed {
        p.line_segment([pos2(cx + r, arc_c.y), pos2(cx + r, body.top())], st);
    }
}

pub fn speaker(p: &Painter, c: Pos2, s: f32, col: Color32, level: f32) {
    let x0 = c.x - s * 0.42;
    p.add(Shape::convex_polygon(
        vec![
            pos2(x0, c.y - s * 0.14),
            pos2(x0 + s * 0.18, c.y - s * 0.14),
            pos2(x0 + s * 0.42, c.y - s * 0.36),
            pos2(x0 + s * 0.42, c.y + s * 0.36),
            pos2(x0 + s * 0.18, c.y + s * 0.14),
            pos2(x0, c.y + s * 0.14),
        ],
        col,
        Stroke::NONE,
    ));
    let st = stroke(col, 1.3);
    if level <= 0.0 {
        let x = c.x + s * 0.18;
        let d = s * 0.16;
        p.line_segment([pos2(x - d, c.y - d), pos2(x + d, c.y + d)], st);
        p.line_segment([pos2(x - d, c.y + d), pos2(x + d, c.y - d)], st);
        return;
    }
    let arcs = if level < 0.5 { 1 } else { 2 };
    for i in 0..arcs {
        let r = s * (0.2 + 0.17 * i as f32);
        let center = pos2(c.x - s * 0.02, c.y);
        let pts: Vec<Pos2> = (0..=10)
            .map(|k| {
                let a = -PI / 3.5 + (2.0 * PI / 3.5) * k as f32 / 10.0;
                center + Vec2::angled(a) * r
            })
            .collect();
        p.add(Shape::line(pts, st));
    }
}

pub fn search(p: &Painter, c: Pos2, s: f32, col: Color32) {
    let r = s * 0.28;
    let cc = pos2(c.x - s * 0.08, c.y - s * 0.08);
    p.circle_stroke(cc, r, stroke(col, 1.3));
    let d = Vec2::splat(r * 0.72);
    p.line_segment([cc + d, cc + d * 2.0], stroke(col, 1.5));
}

pub fn folder_plus(p: &Painter, c: Pos2, s: f32, col: Color32) {
    let r = Rect::from_center_size(c, Vec2::splat(s * 0.72));
    p.rect_stroke(r, 2.0, stroke(col, 1.2), eframe::egui::StrokeKind::Inside);
    let d = s * 0.18;
    p.line_segment([pos2(c.x - d, c.y), pos2(c.x + d, c.y)], stroke(col, 1.2));
    p.line_segment([pos2(c.x, c.y - d), pos2(c.x, c.y + d)], stroke(col, 1.2));
}

pub fn warning(p: &Painter, c: Pos2, s: f32, col: Color32) {
    let h = s * 0.45;
    p.add(Shape::closed_line(
        vec![pos2(c.x, c.y - h), pos2(c.x + h * 1.05, c.y + h * 0.8), pos2(c.x - h * 1.05, c.y + h * 0.8)],
        stroke(col, 1.3),
    ));
    p.line_segment([pos2(c.x, c.y - h * 0.3), pos2(c.x, c.y + h * 0.25)], stroke(col, 1.3));
    p.circle_filled(pos2(c.x, c.y + h * 0.52), 1.0, col);
}

pub fn music_note(p: &Painter, c: Pos2, s: f32, col: Color32) {
    let st = stroke(col, s * 0.07);
    let (l, r) = (c.x - s * 0.18, c.x + s * 0.3);
    let top = c.y - s * 0.38;
    p.line_segment([pos2(l, top + s * 0.08), pos2(l, c.y + s * 0.22)], st);
    p.line_segment([pos2(r, top), pos2(r, c.y + s * 0.14)], st);
    p.add(Shape::convex_polygon(
        vec![pos2(l, top + s * 0.08), pos2(r, top), pos2(r, top + s * 0.12), pos2(l, top + s * 0.2)],
        col,
        Stroke::NONE,
    ));
    p.circle_filled(pos2(l - s * 0.1, c.y + s * 0.26), s * 0.12, col);
    p.circle_filled(pos2(r - s * 0.1, c.y + s * 0.18), s * 0.12, col);
}

pub fn folder(p: &Painter, c: Pos2, s: f32, col: Color32) {
    let r = Rect::from_center_size(pos2(c.x, c.y + s * 0.05), vec2(s * 0.8, s * 0.56));
    p.rect_stroke(r, 2.0, stroke(col, 1.2), eframe::egui::StrokeKind::Inside);
    p.line_segment([pos2(r.left() + 1.0, r.top()), pos2(r.left() + s * 0.12, r.top() - s * 0.1)], stroke(col, 1.2));
    p.line_segment([pos2(r.left() + s * 0.12, r.top() - s * 0.1), pos2(r.left() + s * 0.32, r.top() - s * 0.1)], stroke(col, 1.2));
}

pub fn video(p: &Painter, c: Pos2, s: f32, col: Color32) {
    let r = Rect::from_center_size(pos2(c.x - s * 0.08, c.y), vec2(s * 0.56, s * 0.44));
    p.rect_stroke(r, 2.0, stroke(col, 1.2), eframe::egui::StrokeKind::Inside);
    p.add(Shape::convex_polygon(
        vec![pos2(r.right() + 1.0, c.y), pos2(c.x + s * 0.42, c.y - s * 0.18), pos2(c.x + s * 0.42, c.y + s * 0.18)],
        col,
        Stroke::NONE,
    ));
}
