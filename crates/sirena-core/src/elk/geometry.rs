//! Points and sections of an edge route (elkrb `Geometry::Point`,
//! `Graph::Section`).

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub start_point: Point,
    pub end_point: Point,
    pub bend_points: Vec<Point>,
}
