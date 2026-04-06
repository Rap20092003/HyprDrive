//! Squarified treemap layout (Bruls et al. 2000).
//!
//! Pure geometry — no I/O, no async. Takes a bounding rectangle and
//! weighted items, returns positioned rectangles with near-square aspect ratios.

use serde::{Deserialize, Serialize};

/// Axis-aligned rectangle in screen coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    fn shorter_side(&self) -> f64 {
        self.w.min(self.h)
    }

    fn area(&self) -> f64 {
        self.w * self.h
    }
}

/// Input: an item with a numeric id and a weight (e.g. bytes).
#[derive(Debug, Clone, Copy)]
pub struct TreemapItem {
    pub id: u32,
    pub weight: f64,
}

/// Output: an item positioned inside the treemap bounds.
#[derive(Debug, Clone, Copy)]
pub struct TreemapNode {
    pub id: u32,
    pub rect: Rect,
}

/// Worst aspect ratio given row sum, min area, max area, and side length.
///
/// Optimized: avoids iterating over the row by tracking running min/max/sum.
/// Formula from Bruls et al.: worst = max(s^2 * rmax / sum^2, sum^2 / (s^2 * rmin))
#[inline]
fn worst_ratio_fast(side: f64, row_sum: f64, row_min: f64, row_max: f64) -> f64 {
    if row_sum <= 0.0 || side <= 0.0 {
        return f64::MAX;
    }
    let s2 = side * side;
    let sum2 = row_sum * row_sum;
    let r1 = s2 * row_max / sum2;
    let r2 = sum2 / (s2 * row_min);
    r1.max(r2)
}

/// Lay out a completed row within `bounds`, return the remaining bounds.
fn layout_row(row: &[(u32, f64)], bounds: &Rect, output: &mut Vec<TreemapNode>) -> Rect {
    let row_area: f64 = row.iter().map(|(_, a)| a).sum();

    if bounds.w >= bounds.h {
        // Row is a vertical strip on the left
        let strip_w = row_area / bounds.h;
        let mut y = bounds.y;
        for &(id, area) in row {
            let h = area / strip_w;
            output.push(TreemapNode {
                id,
                rect: Rect {
                    x: bounds.x,
                    y,
                    w: strip_w,
                    h,
                },
            });
            y += h;
        }
        Rect {
            x: bounds.x + strip_w,
            y: bounds.y,
            w: bounds.w - strip_w,
            h: bounds.h,
        }
    } else {
        // Row is a horizontal strip on the top
        let strip_h = row_area / bounds.w;
        let mut x = bounds.x;
        for &(id, area) in row {
            let w = area / strip_h;
            output.push(TreemapNode {
                id,
                rect: Rect {
                    x,
                    y: bounds.y,
                    w,
                    h: strip_h,
                },
            });
            x += w;
        }
        Rect {
            x: bounds.x,
            y: bounds.y + strip_h,
            w: bounds.w,
            h: bounds.h - strip_h,
        }
    }
}

/// Compute a squarified treemap layout.
///
/// Items with weight <= 0 are silently excluded. Returns one `TreemapNode`
/// per valid item, laid out to fill `bounds` proportionally.
///
/// Complexity: O(n log n) sort + O(n) layout.
pub fn squarify(bounds: Rect, items: &[TreemapItem]) -> Vec<TreemapNode> {
    let mut sorted: Vec<(u32, f64)> = items
        .iter()
        .filter(|it| it.weight > 0.0)
        .map(|it| (it.id, it.weight))
        .collect();
    sorted.sort_unstable_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    if sorted.is_empty() || bounds.area() <= 0.0 {
        return Vec::new();
    }

    // Normalize weights to areas in-place
    let total_weight: f64 = sorted.iter().map(|(_, w)| w).sum();
    let scale = bounds.area() / total_weight;
    for item in &mut sorted {
        item.1 *= scale;
    }

    let mut output = Vec::with_capacity(sorted.len());
    let mut remaining = bounds;
    let mut row: Vec<(u32, f64)> = Vec::new();
    let mut row_sum = 0.0_f64;
    let mut row_min = f64::MAX;
    let mut row_max = 0.0_f64;

    for &(id, area) in &sorted {
        let side = remaining.shorter_side();

        if row.is_empty() {
            row.push((id, area));
            row_sum = area;
            row_min = area;
            row_max = area;
        } else {
            let new_sum = row_sum + area;
            let new_min = row_min.min(area);
            let new_max = row_max.max(area);
            let prev_worst = worst_ratio_fast(side, row_sum, row_min, row_max);
            let new_worst = worst_ratio_fast(side, new_sum, new_min, new_max);
            if new_worst <= prev_worst {
                row.push((id, area));
                row_sum = new_sum;
                row_min = new_min;
                row_max = new_max;
            } else {
                remaining = layout_row(&row, &remaining, &mut output);
                row.clear();
                row.push((id, area));
                row_sum = area;
                row_min = area;
                row_max = area;
            }
        }
    }

    if !row.is_empty() {
        layout_row(&row, &remaining, &mut output);
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOUNDS: Rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
    };
    const EPSILON: f64 = 1e-6;

    fn approx_eq(a: f64, b: f64) -> bool {
        (a - b).abs() < EPSILON
    }

    #[test]
    fn empty_input_returns_empty() {
        let result = squarify(BOUNDS, &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn single_item_fills_bounds() {
        let items = [TreemapItem {
            id: 0,
            weight: 100.0,
        }];
        let result = squarify(BOUNDS, &items);
        assert_eq!(result.len(), 1);
        let r = result[0].rect;
        assert!(approx_eq(r.x, 0.0));
        assert!(approx_eq(r.y, 0.0));
        assert!(approx_eq(r.w, 100.0));
        assert!(approx_eq(r.h, 100.0));
    }

    #[test]
    fn two_items_proportional_split() {
        let items = [
            TreemapItem {
                id: 0,
                weight: 75.0,
            },
            TreemapItem {
                id: 1,
                weight: 25.0,
            },
        ];
        let result = squarify(BOUNDS, &items);
        assert_eq!(result.len(), 2);
        let total_area: f64 = result.iter().map(|n| n.rect.w * n.rect.h).sum();
        assert!(approx_eq(total_area, 10000.0));
        let big = result.iter().find(|n| n.id == 0).unwrap();
        assert!(approx_eq(big.rect.w * big.rect.h, 7500.0));
    }

    #[test]
    fn aspect_ratios_below_threshold() {
        let items: Vec<TreemapItem> = (0..20)
            .map(|i| TreemapItem {
                id: i,
                weight: (20 - i) as f64 + 1.0,
            })
            .collect();
        let result = squarify(BOUNDS, &items);
        assert_eq!(result.len(), 20);
        for node in &result {
            let r = &node.rect;
            let ratio = if r.w > r.h { r.w / r.h } else { r.h / r.w };
            assert!(
                ratio < 5.0,
                "aspect ratio {} too high for id={}",
                ratio,
                node.id
            );
        }
    }

    #[test]
    fn total_area_matches_bounds() {
        let items: Vec<TreemapItem> = (0..50)
            .map(|i| TreemapItem {
                id: i,
                weight: (i as f64 + 1.0).powi(2),
            })
            .collect();
        let result = squarify(BOUNDS, &items);
        let total_area: f64 = result.iter().map(|n| n.rect.w * n.rect.h).sum();
        assert!(approx_eq(total_area, BOUNDS.area()));
    }

    #[test]
    fn no_overlap_between_rects() {
        let items: Vec<TreemapItem> = (0..30)
            .map(|i| TreemapItem {
                id: i,
                weight: (30 - i) as f64 * 100.0,
            })
            .collect();
        let result = squarify(BOUNDS, &items);
        for (i, a) in result.iter().enumerate() {
            for b in result.iter().skip(i + 1) {
                let ar = &a.rect;
                let br = &b.rect;
                let no_overlap = ar.x + ar.w <= br.x + EPSILON
                    || br.x + br.w <= ar.x + EPSILON
                    || ar.y + ar.h <= br.y + EPSILON
                    || br.y + br.h <= ar.y + EPSILON;
                assert!(no_overlap, "rects {} and {} overlap", a.id, b.id);
            }
        }
    }

    #[test]
    fn stress_test_1000_items() {
        let items: Vec<TreemapItem> = (0..1000)
            .map(|i| TreemapItem {
                id: i,
                weight: (1000 - i) as f64 + 1.0,
            })
            .collect();
        let result = squarify(BOUNDS, &items);
        assert_eq!(result.len(), 1000);
        let total_area: f64 = result.iter().map(|n| n.rect.w * n.rect.h).sum();
        assert!(approx_eq(total_area, BOUNDS.area()));
    }

    #[test]
    fn zero_and_negative_weights_excluded() {
        let items = [
            TreemapItem {
                id: 0,
                weight: 50.0,
            },
            TreemapItem { id: 1, weight: 0.0 },
            TreemapItem {
                id: 2,
                weight: -10.0,
            },
            TreemapItem {
                id: 3,
                weight: 50.0,
            },
        ];
        let result = squarify(BOUNDS, &items);
        assert_eq!(result.len(), 2);
        let ids: Vec<u32> = result.iter().map(|n| n.id).collect();
        assert!(ids.contains(&0));
        assert!(ids.contains(&3));
    }
}
