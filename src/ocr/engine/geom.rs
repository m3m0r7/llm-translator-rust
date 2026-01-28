use crate::ocr::BBoxPx;

pub(super) fn iou(a: &BBoxPx, b: &BBoxPx) -> f32 {
    let ax2 = a.x + a.w;
    let ay2 = a.y + a.h;
    let bx2 = b.x + b.w;
    let by2 = b.y + b.h;

    let ix1 = a.x.max(b.x);
    let iy1 = a.y.max(b.y);
    let ix2 = ax2.min(bx2);
    let iy2 = ay2.min(by2);

    if ix2 <= ix1 || iy2 <= iy1 {
        return 0.0;
    }
    let inter = (ix2 - ix1) as f32 * (iy2 - iy1) as f32;
    let area_a = (a.w as f32) * (a.h as f32);
    let area_b = (b.w as f32) * (b.h as f32);
    inter / (area_a + area_b - inter).max(1.0)
}

pub(super) fn horizontal_overlap_ratio(a: &BBoxPx, b: &BBoxPx) -> f32 {
    let ax2 = a.x + a.w;
    let bx2 = b.x + b.w;
    let ix1 = a.x.max(b.x);
    let ix2 = ax2.min(bx2);
    if ix2 <= ix1 {
        return 0.0;
    }
    let inter = (ix2 - ix1) as f32;
    inter / (a.w.min(b.w) as f32).max(1.0)
}

pub(super) fn vertical_overlap_ratio(a: &BBoxPx, b: &BBoxPx) -> f32 {
    let ay2 = a.y + a.h;
    let by2 = b.y + b.h;
    let iy1 = a.y.max(b.y);
    let iy2 = ay2.min(by2);
    if iy2 <= iy1 {
        return 0.0;
    }
    let inter = (iy2 - iy1) as f32;
    inter / (a.h.min(b.h) as f32).max(1.0)
}

pub(super) fn union_bbox(a: &BBoxPx, b: &BBoxPx) -> BBoxPx {
    let x1 = a.x.min(b.x);
    let y1 = a.y.min(b.y);
    let x2 = (a.x + a.w).max(b.x + b.w);
    let y2 = (a.y + a.h).max(b.y + b.h);
    BBoxPx {
        x: x1,
        y: y1,
        w: x2 - x1,
        h: y2 - y1,
    }
}
