// geo.rs — ROI geometry helpers for zone-occupancy analytics.
//
// A track's `foot` (normalized image point, 0..1) is "inside" a zone when it
// falls within that zone's polygon (a normalized vertex list). The per-zone
// `gym.equipment_occupied` gauge uses this: a zone counts as occupied when at
// least one live track's foot is inside it.
//
// Algorithm: ray-casting (even-odd rule). Works for convex or concave polygons;
// the vertex list may be open or closed (a trailing duplicate of the first
// vertex is tolerated). Coordinate-system agnostic, so normalized [0,1] image
// space needs no special handling.

/// Ray-casting point-in-polygon test. Returns `false` for a polygon with fewer
/// than 3 vertices (degenerate / empty).
pub fn point_in_polygon(x: f32, y: f32, poly: &[(f32, f32)]) -> bool {
    if poly.len() < 3 {
        return false;
    }
    // PNPOLY (W. R. Franklin) ray-casting / even-odd rule: count how many
    // polygon edges a horizontal ray toward +x from (x, y) crosses. An edge
    // between vertices j and i straddles the horizontal at y when
    // `(yi > y) != (yj > y)`; compute where that edge crosses y and flip
    // `inside` each time the crossing is to the right of x. Closed polygons
    // (trailing vertex == first) add a zero-length edge that the straddle test
    // skips (yi == yj), so they are tolerated.
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        if (yi > y) != (yj > y) {
            let x_cross = (xj - xi) * (y - yi) / (yj - yi) + xi;
            if x < x_cross {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Vec<(f32, f32)> {
        vec![(0.2, 0.2), (0.8, 0.2), (0.8, 0.8), (0.2, 0.8)]
    }

    #[test]
    fn point_inside_square_is_inside() {
        assert!(point_in_polygon(0.5, 0.5, &square()));
    }

    #[test]
    fn points_outside_square_are_outside() {
        let s = square();
        assert!(!point_in_polygon(0.1, 0.1, &s));
        assert!(!point_in_polygon(0.9, 0.9, &s));
        assert!(!point_in_polygon(0.5, 0.05, &s));
        assert!(!point_in_polygon(0.95, 0.5, &s));
    }

    #[test]
    fn triangle_inside_and_outside() {
        let tri = vec![(0.0, 0.0), (1.0, 0.0), (0.5, 1.0)];
        assert!(point_in_polygon(0.5, 0.3, &tri), "centroid region inside");
        assert!(!point_in_polygon(0.9, 0.9, &tri), "outside the hypotenuse");
    }

    #[test]
    fn degenerate_polygons_are_outside() {
        assert!(!point_in_polygon(0.5, 0.5, &[(0.0, 0.0), (1.0, 1.0)]), "2 vertices");
        assert!(!point_in_polygon(0.5, 0.5, &[]), "empty");
        assert!(!point_in_polygon(0.5, 0.5, &[(0.5, 0.5)]), "1 vertex");
    }

    #[test]
    fn closed_polygon_with_repeated_last_vertex_works() {
        // A trailing duplicate of the first vertex (common when zones are drawn
        // closed) must not flip the result.
        let mut s = square();
        s.push((0.2, 0.2));
        assert!(point_in_polygon(0.5, 0.5, &s));
    }

    #[test]
    fn concave_notch_respected() {
        // U-shape: the notch in the middle is outside even though its bbox is
        // inside the polygon's bounds.
        let u = vec![
            (0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.6, 1.0),
            (0.6, 0.4), (0.4, 0.4), (0.4, 1.0), (0.0, 1.0),
        ];
        assert!(point_in_polygon(0.2, 0.2, &u), "inside left arm");
        assert!(point_in_polygon(0.8, 0.8, &u), "inside right arm");
        assert!(!point_in_polygon(0.5, 0.8, &u), "inside the notch — outside");
    }
}
