//! # Conway-Hart Polyhedron Operations
//!
//! This crate implements the [Conway Polyhedron
//! Operators](http://en.wikipedia.org/wiki/Conway_polyhedron_notation)
//! and their extensions by
//! [George W. Hart](http://www.georgehart.com/) and others.
//!
//! The internal representation uses mesh buffers. These need
//! furter preprocessing before they can be sent to a GPU but
//! are almost fine to send to an offline renderer, as-is.
//!
//! See the `playground` example for code on how to do either.
//! ## Example
//! ```
//! use polyhedron_ops::Polyhedron;
//! use std::path::Path;
//!
//! // Conway notation: gapcD
//! let polyhedron =
//!     Polyhedron::dodecahedron()
//!         .chamfer(None, true)
//!         .propellor(None, true)
//!         .ambo(None, true)
//!         .gyro(None, None, true)
//!         .finalize();
//!
//! // Export as ./polyhedron-gapcD.obj
//! # #[cfg(feature = "obj")]
//! # {
//! polyhedron.write_to_obj(&Path::new("."), false);
//! # }
//!```
//! The above code starts from a
//! [dodecahedron](https://en.wikipedia.org/wiki/Dodecahedron) and
//! iteratively applies four operators.
//!
//! The resulting shape is shown below.
//!
//! ![](https://raw.githubusercontent.com/virtualritz/polyhedron-operators/HEAD/gapcD.jpg)
//!
//! ## Cargo Features
//! The crate supports sending data to renderers implementing the
//! [ɴsɪ](https://crates.io/crates/nsi/) API. The function is called
//! [`to_nsi()`](Polyhedron::to_nsi()) and is enabled through the
//! `"nsi"` feature.
//!
//! Output to
//! [Wavefront OBJ](https://en.wikipedia.org/wiki/Wavefront_.obj_file)
//! is supported via the `"obj"` feature which adds the
//! [`write_to_obj()`](Polyhedron::write_to_obj()) function.
//! ```toml
//! [dependencies]
//! polyhedron-ops = { version = "0.1.4", features = [ "nsi", "obj" ] }
//! ```
use clamped::Clamp;
use itertools::Itertools;
#[cfg(feature = "nsi")]
use nsi;
use rayon::prelude::*;
#[cfg(feature = "obj")]
use std::{
    error::Error,
    fs::File,
    io::Write as IoWrite,
    path::{Path, PathBuf},
};
use std::{
    fmt::{Display, Write},
    iter::Iterator,
};

pub type Float = f32;
pub type Index = u32;
pub type Face = Vec<Index>;
pub(crate) type FaceSlice = [Index];
pub type Faces = Vec<Face>;
pub(crate) type FacesSlice = [Face];
pub type FaceSet = Vec<Index>;
pub type Edge = [Index; 2];
pub type Edges = Vec<Edge>;
pub(crate) type _EdgeSlice = [Edge];
pub type Point = ultraviolet::vec::Vec3;
pub type Vector = ultraviolet::vec::Vec3;
pub type Normal = Vector;
#[allow(dead_code)]
pub type Normals = Vec<Normal>;
pub type Points = Vec<Point>;
pub(crate) type PointSlice = [Point];
pub(crate) type PointRefSlice<'a> = [&'a Point];

pub enum NormalType {
    Smooth(Float),
    Flat,
}

pub mod prelude {
    //! Re-exports commonly used types and traits.
    //!
    //! Importing the contents of this module is recommended.
    pub use crate::*;
}

fn format_vec<T: Display>(vector: &[T]) -> String {
    if vector.is_empty() {
        String::new()
    } else {
        let mut string = String::with_capacity(vector.len() * 2);
        if 1 == vector.len() {
            write!(&mut string, "{}", vector[0]).unwrap();
        } else {
            string.push('[');
            write!(&mut string, "{}", vector[0]).unwrap();
            for i in vector.get(1..).unwrap() {
                write!(&mut string, ",{}", i).unwrap();
            }
            string.push(']');
        }
        string
    }
}

#[inline]
fn _to_vadd(points: &PointSlice, v: &Vector) -> Points {
    points.par_iter().map(|p| *p + *v).collect()
}

#[inline]
fn vadd(points: &mut Points, v: &Vector) {
    points.par_iter_mut().for_each(|p| *p += *v);
}

#[inline]
fn centroid(points: &PointSlice) -> Point {
    points
        .iter()
        .fold(Point::zero(), |accumulate, point| accumulate + *point)
        //.into_par_iter()
        //.cloned()
        //.reduce(|| Point::zero(), |accumulate, point| accumulate + point);
        / points.len() as Float
}

#[inline]
fn centroid_ref(points: &PointRefSlice) -> Point {
    points
        .iter()
        .fold(Point::zero(), |accumulate, point| accumulate + **point)
        / points.len() as Float
}

#[inline]
fn ordered_vertex_edges_recurse(
    v: u32,
    vfaces: &FacesSlice,
    face: &FaceSlice,
    k: usize,
) -> Edges {
    if k < vfaces.len() {
        let i = index_of(&v, face).unwrap();
        let j = (i + face.len() - 1) % face.len();
        let edge = [v, face[j]];
        let nface = face_with_edge(&edge, vfaces);
        let mut result = vec![edge];
        result.extend(ordered_vertex_edges_recurse(v, vfaces, &nface, k + 1));
        result
    } else {
        vec![]
    }
}

#[inline]
fn ordered_vertex_edges(v: u32, vfaces: &FacesSlice) -> Edges {
    if vfaces.is_empty() {
        vec![]
    } else {
        let face = &vfaces[0];
        let i = index_of(&v, face).unwrap();
        let j = (i + face.len() - 1) % face.len();
        let edge = [v, face[j]];
        let nface = face_with_edge(&edge, vfaces);
        let mut result = vec![edge];
        result.extend(ordered_vertex_edges_recurse(v, vfaces, &nface, 1));
        result
    }
}

#[inline]
fn distinct_edge(edge: &Edge) -> Edge {
    if edge[0] < edge[1] {
        *edge
    } else {
        let mut e = *edge;
        e.reverse();
        e
    }
}

#[inline]
fn distinct_face_edges(face: &FaceSlice) -> Edges {
    face.iter()
        .cycle()
        .tuple_windows::<(_, _)>()
        .map(|t| {
            if t.0 < t.1 {
                [*t.0, *t.1]
            } else {
                [*t.1, *t.0]
            }
        })
        .take(face.len())
        .collect()
}

#[inline]
fn _to_centroid_points(points: &PointSlice) -> Points {
    _to_vadd(points, &-centroid(points))
}

#[inline]
fn center_on_centroid(points: &mut Points) {
    vadd(points, &-centroid(points));
}

#[inline]
fn vnorm(points: &PointSlice) -> Vec<Float> {
    points
        .par_iter()
        .map(|v| Normal::new(v.x, v.y, v.z).mag())
        .collect()
}
// Was: average_norm
#[inline]
fn _average_magnitude(points: &PointSlice) -> Float {
    vnorm(points).par_iter().sum::<Float>() / points.len() as Float
}

#[inline]
fn max_magnitude(points: &PointSlice) -> Float {
    vnorm(points)
        .into_par_iter()
        .reduce(|| Float::NAN, Float::max)
}

/// Returns a [`Faces`] of faces
/// containing `vertex_number`.
#[inline]
fn vertex_faces(
    vertex_number: Index,
    face_index: &FacesSlice,
) -> Faces {
    face_index
        .par_iter()
        .filter(|face| face.contains(&vertex_number))
        .cloned()
        .collect()
}

/// Returns a [`Vec`] of anticlockwise
/// ordered edges.
fn _ordered_face_edges_(face: &FaceSlice) -> Edges {
    face.iter()
        .cycle()
        .tuple_windows::<(_, _)>()
        .map(|edge| [*edge.0, *edge.1])
        .take(face.len())
        .collect()
}

/// Returns a [`Vec`] of anticlockwise
/// ordered edges.
#[inline]
fn ordered_face_edges(face: &FaceSlice) -> Edges {
    (0..face.len())
        .map(|i| [face[i], face[(i + 1) % face.len()]])
        .collect()
}

#[inline]
fn face_with_edge(edge: &Edge, faces: &FacesSlice) -> Face {
    let result = faces
        .par_iter()
        .filter(|face| ordered_face_edges(face).contains(edge))
        .flatten()
        .cloned()
        .collect();
    result
}

#[inline]
fn index_of<T: PartialEq>(element: &T, list: &[T]) -> Option<usize> {
    list.iter().position(|e| *e == *element)
}

/// Used internally by [`ordered_vertex_faces()`].
#[inline]
fn ordered_vertex_faces_recurse(
    v: Index,
    face_index: &FacesSlice,
    cface: &FaceSlice,
    k: Index,
) -> Faces {
    if (k as usize) < face_index.len() {
        let i = index_of(&v, &cface).unwrap() as i32;
        let j = ((i - 1 + cface.len() as i32) % cface.len() as i32) as usize;
        let edge = [v, cface[j]];
        let mut nfaces = vec![face_with_edge(&edge, face_index)];
        nfaces.extend(ordered_vertex_faces_recurse(
            v,
            face_index,
            &nfaces[0],
            k + 1,
        ));
        nfaces
    } else {
        Faces::new()
    }
}

#[inline]
fn ordered_vertex_faces(
    vertex_number: Index,
    face_index: &FacesSlice,
) -> Faces {
    let mut result = vec![face_index[0].clone()];
    result.extend(ordered_vertex_faces_recurse(
        vertex_number,
        face_index,
        &face_index[0],
        1,
    ));

    result
}

#[inline]
fn edge_length(edge: &Edge, points: &PointSlice) -> Float {
    let edge = vec![edge[0], edge[1]];
    let points = as_points(&edge, points);
    (*points[0] - *points[1]).mag()
}

#[inline]
fn _edge_lengths(edges: &_EdgeSlice, points: &PointSlice) -> Vec<Float> {
    edges
        .par_iter()
        .map(|edge| edge_length(edge, points))
        .collect()
}

#[inline]
fn face_edges(face: &FaceSlice, points: &PointSlice) -> Vec<Float> {
    ordered_face_edges(face)
        .par_iter()
        .map(|edge| edge_length(edge, points))
        .collect()
}

#[inline]
fn _circumscribed_resize(points: &mut Points, radius: Float) {
    center_on_centroid(points);
    let average = _average_magnitude(points);

    points.par_iter_mut().for_each(|v| *v *= radius / average);
}

fn max_resize(points: &mut Points, radius: Float) {
    center_on_centroid(points);
    let max = max_magnitude(points);

    points.par_iter_mut().for_each(|v| *v *= radius / max);
}

#[inline]
fn _project_on_sphere(points: &mut Points, radius: Float) {
    points
        .par_iter_mut()
        .for_each(|point| *point = radius * point.normalized());
}

#[inline]
fn face_irregular_faces_onlyity(
    face: &FaceSlice,
    points: &PointSlice,
) -> Float {
    let lengths = face_edges(face, points);
    // The largest value in lengths or NaN (0./0.) otherwise.
    lengths.par_iter().cloned().reduce(|| Float::NAN, Float::max)
        // divide by the smallest value in lengths or NaN (0./0.) otherwise.
        / lengths.par_iter().cloned().reduce(|| Float::NAN, Float::min)
}

#[inline]
fn as_points<'a>(f: &[Index], points: &'a PointSlice) -> Vec<&'a Point> {
    f.par_iter().map(|index| &points[*index as usize]).collect()
}

#[inline]
fn orthogonal(v0: &Point, v1: &Point, v2: &Point) -> Vector {
    (*v1 - *v0).cross(*v2 - *v1)
}

#[inline]
fn are_collinear(v0: &Point, v1: &Point, v2: &Point) -> bool {
    orthogonal(&v0, &v1, &v2).mag_sq() < 0.0001
}

/// Computes the normal of a face.
/// Tries to do the right thing if the face
/// is non-planar or degenerate.
#[allow(clippy::unnecessary_wraps)]
#[inline]
fn face_normal(points: &PointRefSlice) -> Option<Vector> {
    let mut normal = Vector::zero();
    let mut num_considered_edges = 0;

    points
        .iter()
        .cycle()
        .tuple_windows::<(_, _, _)>()
        .take(points.len())
        // Filter out collinear edge pairs
        .filter(|corner| !are_collinear(&corner.0, &corner.1, &corner.2))
        .for_each(|corner| {
            num_considered_edges += 1;
            normal -= orthogonal(&corner.0, &corner.1, &corner.2).normalized();
        });

    if 0 != num_considered_edges {
        normal /= num_considered_edges as f32;
        Some(normal)
    } else {
        // Total degenerate or zero size face.
        // We just return the normalized vector
        // from the origin to the center of the face.
        Some(centroid_ref(points).normalized())

        // FIXME: this branch should return None.
        // We need a method to cleanup geometry
        // of degenrate faces/edges instead.
    }
}

#[inline]
fn vertex_ids_edge_ref_ref<'a>(
    entries: &[(&'a Edge, Point)],
    offset: Index,
) -> Vec<(&'a Edge, Index)> {
    entries
        .par_iter()
        .enumerate()
        // FIXME swap with next line once rustfmt is fixed.
        //.map(|i| (i.1.0, i.0 + offset))
        .map(|i| (entries[i.0].0, i.0 as Index + offset))
        .collect()
}

#[inline]
fn vertex_ids_ref_ref<'a>(
    entries: &[(&'a FaceSlice, Point)],
    offset: Index,
) -> Vec<(&'a FaceSlice, Index)> {
    entries
        .par_iter()
        .enumerate()
        // FIXME swap with next line once rustfmt is fixed.
        //.map(|i| (i.1.0, i.0 + offset))
        .map(|i| (entries[i.0].0, i.0 as Index + offset))
        .collect()
}

#[allow(clippy::needless_lifetimes)]
#[inline]
fn vertex_ids_ref<'a>(
    entries: &'a [(Face, Point)],
    offset: Index,
) -> Vec<(&'a FaceSlice, Index)> {
    entries
        .par_iter()
        .enumerate()
        // FIXME swap with next line once rustfmt is fixed.
        //.map(|i| (i.1.0, i.0 + offset))
        .map(|i| (entries[i.0].0.as_slice(), i.0 as Index + offset))
        .collect()
}

#[inline]
fn _vertex_ids(entries: &[(Face, Point)], offset: Index) -> Vec<(Face, Index)> {
    entries
        .par_iter()
        .enumerate()
        // FIXME swap with next line once rustfmt is fixed.
        //.map(|i| (i.1.0, i.0 + offset))
        .map(|i| (entries[i.0].0.clone(), i.0 as Index + offset))
        .collect()
}

#[inline]
fn vertex(key: &FaceSlice, entries: &[(&FaceSlice, Index)]) -> Option<Index> {
    match entries.par_iter().find_first(|f| key == f.0) {
        Some(entry) => Some(entry.1),
        None => None,
    }
}

#[inline]
fn vertex_edge(key: &Edge, entries: &[(&Edge, Index)]) -> Option<Index> {
    match entries.par_iter().find_first(|f| key == f.0) {
        Some(entry) => Some(entry.1),
        None => None,
    }
}

#[inline]
fn vertex_values_as_ref<T>(entries: &[(T, Point)]) -> Vec<&Point> {
    entries.iter().map(|e| &e.1).collect()
}

fn vertex_values<T>(entries: &[(T, Point)]) -> Points {
    entries.iter().map(|e| e.1).collect()
}

#[inline]
fn selected_face(face: &FaceSlice, face_arity: Option<&Vec<usize>>) -> bool {
    match face_arity {
        None => true,
        Some(arity) => arity.contains(&face.len()),
    }
}

#[inline]
fn distinct_edges(faces: &FacesSlice) -> Edges {
    faces
        .iter()
        .flat_map(|face| {
            face.iter()
                .cycle()
                // Grab two index entries.
                .tuple_windows::<(_, _)>()
                .filter(|t| t.0 < t.1)
                // Create an edge from them.
                .map(|t| [*t.0, *t.1])
                .take(face.len())
                .collect::<Vec<_>>()
        })
        .collect::<Edges>()
        .into_iter()
        .unique()
        .collect()
}

/// Extend a vector with some element(s)
/// ```
/// extend![..foo, 4, 5, 6]
/// ```
macro_rules! extend {
    (..$v:expr, $($new:expr),*) => {{
        let mut tmp = $v.clone();
        $(
        tmp.push($new);
        )*
        tmp
    }}
}

#[derive(Clone, Debug)]
pub struct Polyhedron {
    points: Points,
    //face_arity: Vec<index>,
    face_index: Faces,
    // This stores a FaceSet for each
    // set of faces belonging to the
    // same operations.
    face_set_index: Vec<FaceSet>,
    name: String,
}

impl Default for Polyhedron {
    fn default() -> Self {
        Self::new()
    }
}

impl Polyhedron {
    pub fn new() -> Self {
        Self {
            points: Vec::new(),
            face_index: Vec::new(),
            face_set_index: Vec::new(),
            name: String::new(),
        }
    }

    pub fn from(
        name: &str,
        points: Points,
        face_index: Faces,
        face_set_index: Option<Vec<FaceSet>>,
    ) -> Self {
        Self {
            points,
            face_index,
            face_set_index: face_set_index.unwrap_or_default(),
            name: name.to_string(),
        }
    }

    #[inline]
    fn points_to_faces(mesh: &Self) -> Faces {
        mesh.points
            .par_iter()
            .enumerate()
            .map(|vertex| {
                // Each old vertex creates a new face.
                ordered_vertex_faces(
                    vertex.0 as Index,
                    &vertex_faces(vertex.0 as Index, &mesh.face_index),
                )
                .iter()
                .map(|original_face|
                    // With vertex faces in left-hand order.
                    index_of(original_face, &mesh.face_index).unwrap() as Index)
                .collect()
            })
            .collect()
    }

    /// Appends indices for newly added faces
    /// as a new FaceSet to the FaceSetIndex.
    fn append_new_face_set(&mut self, size: usize) {
        self.face_set_index
            .append(&mut vec![((self.face_index.len() as u32)
                ..((self.face_index.len() + size) as u32))
                .collect()]);
    }

    /// Creates vertices with valence (aka degree) four. It is also
    /// called
    /// [rectification](https://en.wikipedia.org/wiki/Rectification_(geometry)),
    /// or the
    /// [medial graph](https://en.wikipedia.org/wiki/Medial_graph)
    /// in graph theory.
    #[inline]
    pub fn ambo(
        &mut self,
        ratio: Option<Float>,
        change_name: bool,
    ) -> &mut Self {
        let ratio_ = match ratio {
            Some(r) => r.clamped(0.0, 1.0),
            None => 1. / 2.,
        };

        let edges = distinct_edges(&self.face_index);

        let points: Vec<(&Edge, Point)> = edges
            .par_iter()
            .map(|edge| {
                let edge_points = as_points(edge, &self.points);
                (
                    edge,
                    ratio_ * *edge_points[0] + (1.0 - ratio_) * *edge_points[1],
                )
            })
            .collect();

        let new_ids = vertex_ids_edge_ref_ref(&points, 0);

        let mut face_index: Faces = self
            .face_index
            .par_iter()
            .map(|face| {
                let edges = distinct_face_edges(face);
                let result = edges
                    .iter()
                    .filter_map(|edge| vertex_edge(edge, &new_ids))
                    .collect::<Vec<_>>();
                result
            })
            .collect::<Vec<_>>();

        let mut new_face_index: Faces = self
            .points
            // Each old vertex creates a new face ...
            .par_iter()
            .enumerate()
            .map(|polygon_vertex| {
                let vertex_number = polygon_vertex.0 as Index;
                ordered_vertex_edges(
                    vertex_number,
                    &vertex_faces(vertex_number, &self.face_index),
                )
                .iter()
                .map(|ve| {
                    vertex_edge(&distinct_edge(ve), &new_ids).unwrap() as Index
                })
                .collect::<Vec<_>>()
            })
            .collect();

        face_index.append(&mut new_face_index);

        self.append_new_face_set(face_index.len());

        self.face_index = face_index;
        self.points = vertex_values(&points);

        if change_name {
            let mut params = String::new();
            if let Some(ratio) = ratio {
                write!(&mut params, "{:.2}", ratio).unwrap();
            }
            self.name = format!("a{}{}", params, self.name);
        }

        self
    }

    pub fn bevel(
        &mut self,
        ratio: Option<Float>,
        height: Option<Float>,
        vertex_valence: Option<Vec<usize>>,
        regular_faces_only: bool,
        change_name: bool,
    ) -> &mut Self {
        self.truncate(
            height,
            vertex_valence.clone(),
            regular_faces_only,
            false,
        );
        self.ambo(ratio, false);

        if change_name {
            let mut params = String::new();
            if let Some(ratio) = ratio {
                write!(&mut params, "{:.2}", ratio).unwrap();
            }
            if let Some(height) = height {
                write!(&mut params, "{:.2}", height).unwrap();
            }
            if let Some(vertex_valence) = vertex_valence {
                write!(&mut params, ",{}", format_vec(&vertex_valence))
                    .unwrap();
            }
            if regular_faces_only {
                params.push_str(",{t}");
            }
            self.name = format!("b{}{}", params, self.name);
        }

        self
    }

    pub fn chamfer(
        &mut self,
        ratio: Option<Float>,
        change_name: bool,
    ) -> &mut Self {
        let ratio_ = match ratio {
            Some(r) => r.clamped(0.0, 1.0),
            None => 1. / 2.,
        };

        let new_points: Vec<(Face, Point)> = self
            .face_index
            .par_iter()
            .flat_map(|face| {
                let face_points = as_points(face, &self.points);
                let centroid = centroid_ref(&face_points);
                // println!("{:?}", ep);
                let mut result = Vec::new();
                face.iter().enumerate().for_each(|face_point| {
                    let j = face_point.0;
                    let mut new_face = face.clone();
                    new_face.push(face[j]);
                    result.push((
                        new_face,
                        *face_points[j] + ratio_ * (centroid - *face_points[j]),
                    ))
                });
                result
            })
            .collect();

        let new_ids = vertex_ids_ref(&new_points, self.points_len() as Index);

        let mut face_index: Faces = self
            .face_index
            .iter()
            .map(|face| {
                let mut new_face = Vec::with_capacity(face.len());
                face.iter().for_each(|vertex_key| {
                    let mut face_key = face.clone();
                    face_key.push(*vertex_key);
                    new_face.push(vertex(&face_key, &new_ids).unwrap());
                });
                new_face
            })
            .collect();

        face_index.extend(
            self.face_index
                .par_iter()
                .flat_map(|face| {
                    (0..face.len())
                        .filter(|j| face[*j] < face[(*j + 1) % face.len()])
                        .map(|j| {
                            let a: u32 = face[j];
                            let b: u32 = face[(j + 1) % face.len()];
                            let opposite_face =
                                face_with_edge(&[b, a], &self.face_index);

                            vec![
                                a,
                                vertex(&extend![..opposite_face, a], &new_ids)
                                    .unwrap(),
                                vertex(&extend![..opposite_face, b], &new_ids)
                                    .unwrap(),
                                b,
                                vertex(&extend![..face, b], &new_ids).unwrap(),
                                vertex(&extend![..face, a], &new_ids).unwrap(),
                            ]
                        })
                        .collect::<Faces>()
                })
                .collect::<Faces>(),
        );

        self.append_new_face_set(face_index.len());

        self.face_index = face_index;
        self.points.par_iter_mut().for_each(|point| {
            *point = (1.5 * ratio_) * *point;
        });
        self.points.extend(vertex_values(&new_points));

        if change_name {
            let mut params = String::new();
            if let Some(ratio) = ratio {
                write!(&mut params, "{:.2}", ratio).unwrap();
            }
            self.name = format!("c{}{}", params, self.name);
        }

        self
    }

    /// Replaces each face with a vertex, and each vertex with a face.
    pub fn dual(&mut self, change_name: bool) -> &mut Self {
        let new_points = self
            .face_index
            .par_iter()
            .map(|face| centroid_ref(&as_points(face, &self.points)))
            .collect();

        // FIXME: FaceSetIndex
        self.face_index = Self::points_to_faces(self);
        self.points = new_points;

        if change_name {
            self.name = format!("d{}", self.name);
        }

        self
    }

    pub fn expand(
        &mut self,
        ratio: Option<Float>,
        change_name: bool,
    ) -> &mut Self {
        self.ambo(ratio, false);
        self.ambo(ratio, false);

        if change_name {
            let mut params = String::new();
            if let Some(ratio) = ratio {
                write!(&mut params, "{:.2}", ratio).unwrap();
            }
            self.name = format!("e{}{}", params, self.name);
        }

        self
    }

    pub fn gyro(
        &mut self,
        ratio: Option<f32>,
        height: Option<f32>,
        change_name: bool,
    ) -> &mut Self {
        let ratio_ = match ratio {
            Some(r) => r.clamped(0.0, 1.0),
            None => 1. / 3.,
        };
        let height_ = height.unwrap_or(0.);

        // Retain original points, add face centroids and directed
        // edge points each N-face becomes N pentagons.
        let mut new_points: Vec<(&FaceSlice, Point)> = self
            .face_index
            .par_iter()
            .map(|face| {
                let fp = as_points(face, &self.points);
                (
                    face.as_slice(),
                    centroid_ref(&fp).normalized()
                        + face_normal(&fp).unwrap() * height_,
                )
            })
            .collect();

        let edges = self.to_edges();
        let reversed_edges: Edges =
            edges.par_iter().map(|edge| [edge[1], edge[0]]).collect();

        let new_points2: Vec<(&FaceSlice, Point)> = edges
            .par_iter()
            .enumerate()
            .flat_map(|edge| {
                let edge_points = as_points(edge.1, &self.points);
                // println!("{:?}", ep);
                vec![
                    (
                        &edge.1[..],
                        *edge_points[0]
                            + ratio_ * (*edge_points[1] - *edge_points[0]),
                    ),
                    (
                        &reversed_edges[edge.0][..],
                        *edge_points[1]
                            + ratio_ * (*edge_points[0] - *edge_points[1]),
                    ),
                ]
            })
            .collect();

        new_points.extend(new_points2);
        //  2 points per edge

        let new_ids =
            vertex_ids_ref_ref(&new_points, self.points_len() as Index);

        self.points.extend(vertex_values_as_ref(&new_points));

        self.face_index = self
            .face_index
            .par_iter()
            .flat_map(|face| {
                (0..face.len())
                    .map(|j| {
                        let a = face[j];
                        let b = face[(j + 1) % face.len()];
                        let z = face[(j + face.len() - 1) % face.len()];
                        let eab = vertex(&[a, b], &new_ids).unwrap();
                        let eza = vertex(&[z, a], &new_ids).unwrap();
                        let eaz = vertex(&[a, z], &new_ids).unwrap();
                        let centroid = vertex(face, &new_ids).unwrap();
                        vec![a, eab, centroid, eza, eaz]
                    })
                    .collect::<Faces>()
            })
            .collect();

        if change_name {
            let mut params = String::new();
            if let Some(ratio) = ratio {
                write!(&mut params, "{:.2}", ratio).unwrap();
            }
            if let Some(height) = height {
                write!(&mut params, ",{:.2}", height).unwrap();
            }
            self.name = format!("g{}{}", params, self.name);
        }

        self
    }

    /// Creates quadrilateral faces around each original edge.
    /// Original edges are discarded.
    /// # Arguments
    /// * `ratio` - the ratio at which the adjacent edges gets split.
    ///             Will be clamped to `[0,1]`. Default value is `0.5`.
    pub fn join(
        &mut self,
        ratio: Option<Float>,
        change_name: bool,
    ) -> &mut Self {
        self.dual(false);
        self.ambo(ratio, false);
        self.dual(false);

        if change_name {
            let mut params = String::new();
            if let Some(ratio) = ratio {
                write!(&mut params, "{:.2}", ratio).unwrap();
            }
            self.name = format!("j{}{}", params, self.name);
        }

        self
    }

    /// Splits each face into triangles, one for each edge,
    /// which extend to the face centroid. Existimg points
    /// are retained.
    /// # Arguments
    /// * `height` - An offset to add to the face centroid point along the
    ///              face normal.
    /// * `face_arity` - Only faces matching the given arities will be
    ///                  affected.
    /// * `regular_faces_only` - Only faces whose edges are 90% the same length,
    ///               within the same face, are affected.
    pub fn kis(
        &mut self,
        height: Option<Float>,
        face_arity: Option<Vec<usize>>,
        regular_faces_only: bool,
        change_name: bool,
    ) -> &mut Self {
        let height_ = height.unwrap_or(0.);

        let new_points: Vec<(&FaceSlice, Point)> = self
            .face_index
            .par_iter()
            .filter(|face| {
                selected_face(face, face_arity.as_ref()) && !regular_faces_only
                    || ((face_irregular_faces_onlyity(face, &self.points)
                        - 1.0)
                        .abs()
                        < 0.1)
            })
            .map(|face| {
                let fp = as_points(face, &self.points);
                (
                    face.as_slice(),
                    centroid_ref(&fp) + face_normal(&fp).unwrap() * height_,
                )
            })
            .collect();

        let new_ids =
            vertex_ids_ref_ref(&new_points, self.points.len() as Index);

        self.points.extend(vertex_values_as_ref(&new_points));

        self.face_index = self
            .face_index
            .par_iter()
            .flat_map(|face: &Face| match vertex(face, &new_ids) {
                Some(centroid) => (0..face.len())
                    .map(|j| {
                        vec![
                            face[j],
                            face[(j + 1) % face.len()],
                            centroid as Index,
                        ]
                    })
                    .collect(),
                None => vec![face.clone()],
            })
            .collect();

        if change_name {
            let mut params = String::new();
            if let Some(height) = height {
                write!(&mut params, "{:.2}", height).unwrap();
            }
            if let Some(face_arity) = face_arity {
                write!(&mut params, ",{:.2}", format_vec(&face_arity)).unwrap();
            }
            self.name = format!("k{}{}", params, self.name);
        }

        self
    }

    fn _inset(&self) {
        self._loft()
    }

    fn _loft(&self) {
        // FIXME
    }

    pub fn medial(
        &mut self,
        ratio: Option<Float>,
        height: Option<Float>,
        vertex_valence: Option<Vec<usize>>,
        regular_faces_only: bool,
        change_name: bool,
    ) -> &mut Self {
        self.dual(false);
        self.truncate(
            height,
            vertex_valence.clone(),
            regular_faces_only,
            false,
        );
        self.ambo(ratio, false);

        if change_name {
            let mut params = String::new();
            if let Some(ratio) = ratio {
                write!(&mut params, "{:.2}", ratio).unwrap();
            }
            if let Some(height) = height {
                write!(&mut params, "{:.2}", height).unwrap();
            }
            if let Some(vertex_valence) = vertex_valence {
                write!(&mut params, ",{}", format_vec(&vertex_valence))
                    .unwrap();
            }
            if regular_faces_only {
                params.push_str(",{t}");
            }
            self.name = format!("M{}{}", params, self.name);
        }

        self
    }

    pub fn meta(
        &mut self,
        ratio: Option<Float>,
        height: Option<Float>,
        vertex_valence: Option<Vec<usize>>,
        regular_faces_only: bool,
        change_name: bool,
    ) -> &mut Self {
        self.kis(
            height,
            match vertex_valence {
                // By default meta works on verts.
                // of valence three.
                None => Some(vec![3]),
                _ => vertex_valence.clone(),
            },
            regular_faces_only,
            false,
        );
        self.join(ratio, false);

        if change_name {
            let mut params = String::new();
            if let Some(ratio) = ratio {
                write!(&mut params, "{:.2}", ratio).unwrap();
            }
            if let Some(height) = height {
                write!(&mut params, "{:.2}", height).unwrap();
            }
            if let Some(vertex_valence) = vertex_valence {
                write!(&mut params, ",{}", format_vec(&vertex_valence))
                    .unwrap();
            }
            if regular_faces_only {
                params.push_str(",{t}");
            }
            self.name = format!("m{}{}", params, self.name);
        }

        self
    }

    pub fn needle(
        &mut self,
        height: Option<Float>,
        vertex_valence: Option<Vec<usize>>,
        regular_faces_only: bool,
        change_name: bool,
    ) -> &mut Self {
        self.dual(false);
        self.truncate(
            height,
            vertex_valence.clone(),
            regular_faces_only,
            false,
        );

        if change_name {
            let mut params = String::new();
            if let Some(height) = height {
                write!(&mut params, "{:.2}", height).unwrap();
            }
            if let Some(vertex_valence) = vertex_valence {
                write!(&mut params, ",{}", format_vec(&vertex_valence))
                    .unwrap();
            }
            if regular_faces_only {
                params.push_str(",{t}");
            }
            self.name = format!("n{}{}", params, self.name);
        }

        self
    }

    pub fn ortho(
        &mut self,
        ratio: Option<Float>,
        change_name: bool,
    ) -> &mut Self {
        self.join(ratio, false);
        self.join(ratio, false);

        if change_name {
            let mut params = String::new();
            if let Some(ratio) = ratio {
                write!(&mut params, "{:.2}", ratio).unwrap();
            }
            self.name = format!("o{}{}", params, self.name);
        }

        self
    }

    pub fn propellor(
        &mut self,
        ratio: Option<Float>,
        change_name: bool,
    ) -> &mut Self {
        let ratio_ = match ratio {
            Some(r) => r.clamped(0.0, 1.0),
            None => 1. / 3.,
        };

        let edges = self.to_edges();
        let reversed_edges: Edges =
            edges.iter().map(|edge| [edge[1], edge[0]]).collect();

        let new_points = edges
            .iter()
            .zip(reversed_edges.iter())
            .flat_map(|(edge, reversed_edge)| {
                let edge_points = as_points(edge, &self.points);
                vec![
                    (
                        edge,
                        *edge_points[0]
                            + ratio_ * (*edge_points[1] - *edge_points[0]),
                    ),
                    (
                        reversed_edge,
                        *edge_points[1]
                            + ratio_ * (*edge_points[0] - *edge_points[1]),
                    ),
                ]
            })
            .collect::<Vec<_>>();

        let new_ids =
            vertex_ids_edge_ref_ref(&new_points, self.points_len() as Index);

        let mut face_index: Faces = self
            .face_index
            .iter()
            .map(|face| {
                (0..face.len())
                    .map(|j| {
                        vertex_edge(
                            &[face[j], face[(j + 1) % face.len()]],
                            &new_ids,
                        )
                        .unwrap()
                    })
                    .collect()
            })
            .collect();

        face_index.extend(
            self.face_index
                .iter()
                .flat_map(|face| {
                    (0..face.len())
                        .map(|j| {
                            let a = face[j];
                            let b = face[(j + 1) % face.len()];
                            let z = face[(j + face.len() - 1) % face.len()];
                            let eab = vertex_edge(&[a, b], &new_ids).unwrap();
                            let eba = vertex_edge(&[b, a], &new_ids).unwrap();
                            let eza = vertex_edge(&[z, a], &new_ids).unwrap();
                            vec![a, eba, eab, eza]
                        })
                        .collect::<Faces>()
                })
                .collect::<Faces>(),
        );

        self.face_index = face_index;
        self.points.extend(vertex_values_as_ref(&new_points));

        if change_name {
            let mut params = String::new();
            if let Some(ratio) = ratio {
                write!(&mut params, "{:.2}", ratio).unwrap();
            }
            self.name = format!("p{}{}", params, self.name);
        }

        self
    }

    pub fn quinto(
        &mut self,
        height: Option<Float>,
        change_name: bool,
    ) -> &mut Self {
        let height_ = match height {
            Some(h) => {
                if h < 0.0 {
                    0.0
                } else {
                    h
                }
            }
            None => 0.5,
        };

        let mut new_points: Vec<(Face, Point)> = self.to_edges()
            .par_iter()
            .map(|edge| {
                let edge_points = as_points(edge, &self.points);
                (edge.to_vec(), height_ * (*edge_points[0] + *edge_points[1]))
            })
            .collect();

        new_points.extend(
            self.face_index
                .par_iter()
                .flat_map(|face| {
                    let edge_points = as_points(face, &self.points);
                    let centroid = centroid_ref(&edge_points);
                    (0..face.len())
                        .map(|i| {
                            (
                                extend![..face, i as u32],
                                (*edge_points[i]
                                    + *edge_points[(i + 1) % face.len()]
                                    + centroid)
                                    / 3.,
                            )
                        })
                        .collect::<Vec<(Face, Point)>>()
                })
                .collect::<Vec<(Face, Point)>>(),
        );

        let new_ids = vertex_ids_ref(&new_points, self.points_len() as u32);

        let mut face_index: Faces = self
            .face_index
            .par_iter()
            .map(|face| {
                (0..face.len())
                    .map(|face_vertex| {
                        vertex(&extend![..face, face_vertex as u32], &new_ids)
                            .unwrap()
                    })
                    .collect()
            })
            .collect();

        face_index.extend(
            self.face_index
                .par_iter()
                .flat_map(|face| {
                    (0..face.len())
                        .map(|i| {
                            let v = face[i];
                            let e0 = [
                                face[(i + face.len() - 1) % face.len()],
                                face[i],
                            ];
                            let e1 = [face[i], face[(i + 1) % face.len()]];
                            let e0p =
                                vertex(&distinct_edge(&e0), &new_ids).unwrap();
                            let e1p =
                                vertex(&distinct_edge(&e1), &new_ids).unwrap();
                            let iv0 = vertex(
                                &extend![
                                    ..face,
                                    ((i + face.len() - 1) % face.len()) as u32
                                ],
                                &new_ids,
                            )
                            .unwrap();
                            let iv1 =
                                vertex(&extend![..face, i as u32], &new_ids)
                                    .unwrap();
                            vec![v, e1p, iv1, iv0, e0p]
                        })
                        .collect::<Faces>()
                })
                .collect::<Faces>(),
        );

        self.face_index = face_index;
        self.points.extend(vertex_values_as_ref(&new_points));

        if change_name {
            let mut params = String::new();
            if let Some(h) = height {
                write!(&mut params, "{:.2}", h).unwrap();
            }
            self.name = format!("q{}{}", params, self.name);
        }

        self
    }

    pub fn reflect(&mut self, change_name: bool) -> &mut Self {
        self.points = self
            .points
            .par_iter()
            .map(|v| Point::new(v.x, -v.y, v.z))
            .collect();
        self.reverse();

        if change_name {
            self.name = format!("r{}", self.name);
        }

        self
    }

    pub fn snub(
        &mut self,
        ratio: Option<Float>,
        height: Option<Float>,
        change_name: bool,
    ) -> &mut Self {
        self.dual(false);
        self.gyro(ratio, height, false);
        self.dual(false);

        if change_name {
            let mut params = String::new();
            if let Some(ratio) = ratio {
                write!(&mut params, "{:.2}", ratio).unwrap();
            }
            if let Some(height) = height {
                write!(&mut params, ",{:.2}", height).unwrap();
            }
            self.name = format!("s{}{}", params, self.name);
        }

        self
    }

    pub fn truncate(
        &mut self,
        height: Option<Float>,
        vertex_valence: Option<Vec<usize>>,
        regular_faces_only: bool,
        change_name: bool,
    ) -> &mut Self {
        self.dual(false);
        self.kis(height, vertex_valence.clone(), regular_faces_only, false);
        self.dual(false);

        if change_name {
            let mut params = String::new();
            if let Some(height) = height {
                write!(&mut params, "{:.2}", height).unwrap();
            }
            if let Some(vertex_valence) = vertex_valence {
                write!(&mut params, ",{}", format_vec(&vertex_valence))
                    .unwrap();
            }
            if regular_faces_only {
                params.push_str(",{t}");
            }
            self.name = format!("t{}{}", params, self.name);
        }

        self
    }

    pub fn whirl(
        &mut self,
        ratio: Option<Float>,
        height: Option<Float>,
        change_name: bool,
    ) -> &mut Self {
        let ratio_ = match ratio {
            Some(r) => r.clamped(0.0, 1.0),
            None => 1. / 3.,
        };
        let height_ = height.unwrap_or(0.);

        let mut new_points: Vec<(Face, Point)> = self
            .face_index
            .iter()
            .flat_map(|face| {
                let face_points = as_points(face, &self.points);
                let center = centroid_ref(&face_points)
                    + face_normal(&face_points).unwrap() * height_;
                face.iter()
                    .enumerate()
                    .map(|v| {
                        let edge_points = [
                            face_points[v.0],
                            face_points[(v.0 + 1) % face.len()],
                        ];
                        let middle: ultraviolet::vec::Vec3 = *edge_points[0]
                            + ratio_ * (*edge_points[1] - *edge_points[0]);
                        (
                            extend![..face, *v.1],
                            middle + ratio_ * (center - middle),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .collect();

        let edges = self.to_edges();

        let new_points2: Vec<(Face, Point)> = edges
            .par_iter()
            .flat_map(|edge| {
                let edge_points = as_points(edge, &self.points);
                vec![
                    (
                        edge.to_vec(),
                        *edge_points[0]
                            + ratio_ * (*edge_points[1] - *edge_points[0]),
                    ),
                    (
                        vec![edge[1], edge[0]],
                        *edge_points[1]
                            + ratio_ * (*edge_points[0] - *edge_points[1]),
                    ),
                ]
            })
            .collect();

        new_points.extend(new_points2);

        let new_ids = vertex_ids_ref(&new_points, self.points_len() as Index);

        let mut face_index: Faces = self
            .face_index
            .par_iter()
            .flat_map(|face| {
                (0..face.len())
                    .map(|j| {
                        let a = face[j];
                        let b = face[(j + 1) % face.len()];
                        let c = face[(j + 2) % face.len()];
                        let eab = vertex(&[a, b], &new_ids).unwrap();
                        let eba = vertex(&[b, a], &new_ids).unwrap();
                        let ebc = vertex(&[b, c], &new_ids).unwrap();
                        let mut mid = face.clone();
                        mid.push(a);
                        let mida = vertex(&mid, &new_ids).unwrap();
                        mid.pop();
                        mid.push(b);
                        let midb = vertex(&mid, &new_ids).unwrap();
                        vec![eab, eba, b, ebc, midb, mida]
                    })
                    .collect::<Faces>()
            })
            .collect();

        face_index.extend(
            self.face_index
                .par_iter()
                .map(|face| {
                    let mut new_face = face.clone();
                    face.iter()
                        .map(|a| {
                            new_face.push(*a);
                            let result = vertex(&new_face, &new_ids).unwrap();
                            new_face.pop();
                            result
                        })
                        .collect()
                })
                .collect::<Faces>(),
        );

        self.append_new_face_set(face_index.len() - self.face_index.len());

        self.points.extend(vertex_values(&new_points));
        self.face_index = face_index;

        if change_name {
            let mut params = String::new();
            if let Some(ratio) = ratio {
                write!(&mut params, "{:.2}", ratio).unwrap();
            }
            if let Some(height) = height {
                write!(&mut params, ",{:.2}", height).unwrap();
            }
            self.name = format!("w{}{}", params, self.name);
        }

        self
    }

    pub fn zip(
        &mut self,
        height: Option<Float>,
        vertex_valence: Option<Vec<usize>>,
        regular_faces_only: bool,
        change_name: bool,
    ) -> &mut Self {
        self.dual(false);
        self.kis(height, vertex_valence.clone(), regular_faces_only, false);

        if change_name {
            let mut params = String::new();
            if let Some(height) = height {
                write!(&mut params, "{:.2}", height).unwrap();
            }
            if let Some(vertex_valence) = vertex_valence {
                write!(&mut params, ",{}", format_vec(&vertex_valence))
                    .unwrap();
            }
            if regular_faces_only {
                params.push_str(",{t}");
            }
            self.name = format!("z{}{}", params, self.name);
        }

        self
    }

    /// Reverses the winding order of faces.
    /// Clockwise(default) becomes counter-clockwise and vice versa.
    pub fn reverse(&mut self) -> &mut Self {
        self.face_index
            .par_iter_mut()
            .for_each(|face| face.reverse());

        self
    }

    /// Returns the name of this polyhedron.
    /// This can be used to reconstruct the polyhedron
    /// using Polyhedron::from<&str>().
    #[inline]
    pub fn name(&self) -> &String {
        &self.name
    }

    #[inline]
    pub fn points_len(&self) -> usize {
        self.points.len()
    }

    #[inline]
    pub fn points(&self) -> &Points {
        &self.points
    }

    pub fn faces(&self) -> &Faces {
        &self.face_index
    }

    #[inline]
    pub fn normalize(&mut self) {
        max_resize(&mut self.points, 1.);
    }

    /// Computer the edges of the polyhedron.
    #[inline]
    pub fn to_edges(&self) -> Edges {
        distinct_edges(&self.face_index)
    }

    pub fn normals(&self, normal_type: NormalType) -> Normals {
        match normal_type {
            NormalType::Smooth(_angle) => vec![],
            NormalType::Flat => self
                .face_index
                .par_iter()
                .flat_map(|f| {
                    f.iter()
                        // Cycle forever.
                        .cycle()
                        // Start at 3-tuple belonging to the
                        // face's last vertex.
                        .skip(f.len() - 1)
                        // Grab the next three vertex index
                        // entries.
                        .tuple_windows::<(_, _, _)>()
                        // Create a normal from that
                        .map(|t| {
                            -orthogonal(
                                &self.points[*t.0 as usize],
                                &self.points[*t.1 as usize],
                                &self.points[*t.2 as usize],
                            )
                            .normalized()
                        })
                        .take(f.len())
                        .collect::<Normals>()
                })
                .collect(),
            /*NormalType::Flat => self
            .face_index
            .par_iter()
            .for_each(|f| {
                normals.extend(
                    f.par_iter()
                        // Cycle forever.
                        .cycle()
                        // Start at 3-tuple belonging to the
                        // face's last vertex.
                        .skip(f.len() - 1)
                        // Grab the next three vertex index
                        // entries.
                        .tuple_windows::<(_, _, _)>()
                        // Create a normal from that
                        .for_each(|t| {
                            -orthogonal(
                                &self.points[*t.0 as usize],
                                &self.points[*t.1 as usize],
                                &self.points[*t.2 as usize],
                            )
                            .normalize()
                        })
                        .take(f.len())
                        .collect::<Normals>(),
                );
                face_index.extend(f.par_iter())
            })
            .flatten()
            .collect(),*/
        }
    }

    #[inline]
    pub fn triangulate(&mut self, shortest: bool) -> &mut Self {
        self.face_index = self
            .face_index
            .iter()
            .flat_map(|face| match face.len() {
                // Bitriangulate quadrilateral faces
                // use shortest diagonal so triangles are
                // most nearly equilateral.
                4 => {
                    let p = as_points(face, &self.points);

                    if shortest
                        == ((*p[0] - *p[2]).mag_sq() < (*p[1] - *p[3]).mag_sq())
                    {
                        vec![
                            vec![face[0], face[1], face[2]],
                            vec![face[0], face[2], face[3]],
                        ]
                    } else {
                        vec![
                            vec![face[1], face[2], face[3]],
                            vec![face[1], face[3], face[0]],
                        ]
                    }
                }
                5 => vec![
                    vec![face[0], face[1], face[4]],
                    vec![face[1], face[2], face[4]],
                    vec![face[4], face[2], face[3]],
                ],
                _ => {
                    let a = face[0];
                    let mut bb = face[1];
                    face.iter()
                        .skip(2)
                        .map(|c| {
                            let b = bb;
                            bb = *c;
                            vec![a, b, *c]
                        })
                        .collect()
                } //_ => vec![face.clone()],
            })
            .collect();

        self
    }

    /// Turns the builder into a final object.
    pub fn finalize(&self) -> Self {
        self.clone()
    }

    /// Sends the polyhedron to the specified
    /// [ɴsɪ](https:://crates.io/crates/nsi) context.
    /// # Arguments
    /// * `crease_hardness` - The hardness of edges.
    ///
    /// * `corner_hardness` - The hardness of vertices.
    ///
    /// * `smooth_corners` - Whether to keep corners where more than
    ///     two edges meet smooth. When set to `false` these
    ///     automatically form a hard corner with the same hardness
    ///     as `crease_hardness`..
    #[cfg(feature = "nsi")]
    pub fn to_nsi(
        &self,
        ctx: &nsi::Context,
        crease_hardness: Option<f32>,
        corner_hardness: Option<f32>,
        smooth_corners: Option<bool>,
    ) -> String {
        // Create a new mesh node.
        ctx.create(self.name.clone(), nsi::NodeType::Mesh, &[]);

        // Flatten point vector.
        // Fast, unsafe version. May exploce on some platforms.
        // If so, use commented out code below instead.
        let positions = unsafe {
            std::slice::from_raw_parts(
                self.points.as_ptr().cast::<Float>(),
                3 * self.points_len(),
            )
        };

        /*
        let positions: FlatPoints = self
            .points
            .into_par_iter()
            .flat_map(|p3| once(p3.x).chain(once(p3.y)).chain(once(p3.z)))
            .collect();
        */

        let face_arity = self
            .face_index
            .par_iter()
            .map(|face| face.len() as u32)
            .collect::<Vec<_>>();

        let face_index = self.face_index.concat();

        ctx.set_attribute(
            self.name.clone(),
            &[
                // Positions.
                nsi::points!("P", positions),
                // Index into the position array.
                nsi::unsigneds!("P.indices", &face_index),
                // Arity of each face.
                nsi::unsigneds!("nvertices", &face_arity),
                // Render this as a C-C subdivison surface.
                nsi::string!("subdivision.scheme", "catmull-clark"),
            ],
        );

        let crease_hardness = match crease_hardness {
            Some(h) => h,
            // Default: semi sharp creases.
            None => 10.,
        };

        // Crease each of our edges a bit?
        if 0.0 != crease_hardness {
            let edges = self
                .to_edges()
                .into_par_iter()
                .map(|edge| edge.to_vec())
                .flatten()
                .collect::<Vec<_>>();
            ctx.set_attribute(
                self.name.clone(),
                &[
                    nsi::unsigneds!("subdivision.creasevertices", &edges),
                    nsi::floats!(
                        "subdivision.creasesharpness",
                        &vec![crease_hardness; edges.len()]
                    ),
                ],
            );
        }

        match corner_hardness {
            Some(hardness) => {
                if 0.0 < hardness {
                    let corners = self
                        .points
                        .par_iter()
                        .enumerate()
                        .map(|(i, _)| i as u32)
                        .collect::<Vec<_>>();
                    ctx.set_attribute(
                        self.name.clone(),
                        &[
                            nsi::unsigneds!(
                                "subdivision.cornervertices",
                                &corners
                            ),
                            nsi::floats!(
                                "subdivision.cornersharpness",
                                &vec![hardness; corners.len()]
                            ),
                        ],
                    );
                }
            }

            // Have the renderer semi create sharp corners automagically.
            None => ctx.set_attribute(
                self.name.clone(),
                &[
                    // Disabling below flag activates the specific deRose
                    // extensions for the C-C creasing algorithm
                    // that causes any vertex with where more then three
                    // creased edges meet to forma a corner.
                    // See fig. 8c/d in this paper:
                    // http://graphics.pixar.com/people/derose/publications/Geri/paper.pdf
                    nsi::unsigned!(
                        "subdivision.smoothcreasecorners",
                        smooth_corners.unwrap_or(false) as _
                    ),
                ],
            ),
        };

        self.name.clone()
    }

    /// Write the polyhedron to a
    /// [Wavefront OBJ](https://en.wikipedia.org/wiki/Wavefront_.obj_file)
    /// file.
    ///
    /// The [`name`](Polyhedron::name()) of the polyhedron is appended to the given
    /// `destination` and postfixed with the extension `.obj`.
    ///
    /// Depending on the target coordinate system (left- or right
    /// handed) the mesh’s winding order can be reversed with the
    /// `reverse_face_winding` flag.
    ///
    /// The return value, on success, is the final, complete path of
    /// the OBJ file.
    #[cfg(feature = "obj")]
    pub fn write_to_obj(
        &self,
        destination: &Path,
        reverse_winding: bool,
    ) -> Result<PathBuf, Box<dyn Error>> {
        let path = destination.join(format!("polyhedron-{}.obj", self.name));
        let mut file = File::create(path.clone())?;

        writeln!(file, "o {}", self.name)?;

        for vertex in &self.points {
            writeln!(file, "v {} {} {}", vertex.x, vertex.y, vertex.z)?;
        }

        match reverse_winding {
            true => {
                for face in &self.face_index {
                    write!(file, "f")?;
                    for vertex_index in face.iter().rev() {
                        write!(file, " {}", vertex_index + 1)?;
                    }
                    writeln!(file)?;
                }
            }
            false => {
                for face in &self.face_index {
                    write!(file, "f")?;
                    for vertex_index in face {
                        write!(file, " {}", vertex_index + 1)?;
                    }
                    writeln!(file)?;
                }
            }
        };

        file.flush()?;

        Ok(path)
    }

    pub fn tetrahedron() -> Self {
        let c0 = 1.0;

        Self {
            points: vec![
                Point::new(c0, c0, c0),
                Point::new(c0, -c0, -c0),
                Point::new(-c0, c0, -c0),
                Point::new(-c0, -c0, c0),
            ],
            face_index: vec![
                vec![2, 1, 0],
                vec![3, 2, 0],
                vec![1, 3, 0],
                vec![2, 3, 1],
            ],
            face_set_index: vec![(0..4).collect()],
            name: String::from("T"),
        }
    }

    #[inline]
    pub fn cube() -> Self {
        Self::hexahedron()
    }

    pub fn hexahedron() -> Self {
        let c0 = 1.0;

        Self {
            points: vec![
                Point::new(c0, c0, c0),
                Point::new(c0, c0, -c0),
                Point::new(c0, -c0, c0),
                Point::new(c0, -c0, -c0),
                Point::new(-c0, c0, c0),
                Point::new(-c0, c0, -c0),
                Point::new(-c0, -c0, c0),
                Point::new(-c0, -c0, -c0),
            ],
            face_index: vec![
                vec![4, 5, 1, 0],
                vec![2, 6, 4, 0],
                vec![1, 3, 2, 0],
                vec![6, 2, 3, 7],
                vec![5, 4, 6, 7],
                vec![3, 1, 5, 7],
            ],
            face_set_index: vec![(0..6).collect()],
            name: String::from("C"),
        }
    }

    pub fn octahedron() -> Self {
        let c0 = 0.707_106_77;

        Self {
            points: vec![
                Point::new(0.0, 0.0, c0),
                Point::new(0.0, 0.0, -c0),
                Point::new(c0, 0.0, 0.0),
                Point::new(-c0, 0.0, 0.0),
                Point::new(0.0, c0, 0.0),
                Point::new(0.0, -c0, 0.0),
            ],
            face_index: vec![
                vec![4, 2, 0],
                vec![3, 4, 0],
                vec![5, 3, 0],
                vec![2, 5, 0],
                vec![5, 2, 1],
                vec![3, 5, 1],
                vec![4, 3, 1],
                vec![2, 4, 1],
            ],
            face_set_index: vec![(0..8).collect()],
            name: String::from("O"),
        }
    }

    pub fn dodecahedron() -> Self {
        let c0 = 0.809_017;
        let c1 = 1.309_017;

        Self {
            points: vec![
                Point::new(0.0, 0.5, c1),
                Point::new(0.0, 0.5, -c1),
                Point::new(0.0, -0.5, c1),
                Point::new(0.0, -0.5, -c1),
                Point::new(c1, 0.0, 0.5),
                Point::new(c1, 0.0, -0.5),
                Point::new(-c1, 0.0, 0.5),
                Point::new(-c1, 0.0, -0.5),
                Point::new(0.5, c1, 0.0),
                Point::new(0.5, -c1, 0.0),
                Point::new(-0.5, c1, 0.0),
                Point::new(-0.5, -c1, 0.0),
                Point::new(c0, c0, c0),
                Point::new(c0, c0, -c0),
                Point::new(c0, -c0, c0),
                Point::new(c0, -c0, -c0),
                Point::new(-c0, c0, c0),
                Point::new(-c0, c0, -c0),
                Point::new(-c0, -c0, c0),
                Point::new(-c0, -c0, -c0),
            ],
            face_index: vec![
                vec![12, 4, 14, 2, 0],
                vec![16, 10, 8, 12, 0],
                vec![2, 18, 6, 16, 0],
                vec![17, 10, 16, 6, 7],
                vec![19, 3, 1, 17, 7],
                vec![6, 18, 11, 19, 7],
                vec![15, 3, 19, 11, 9],
                vec![14, 4, 5, 15, 9],
                vec![11, 18, 2, 14, 9],
                vec![8, 10, 17, 1, 13],
                vec![5, 4, 12, 8, 13],
                vec![1, 3, 15, 5, 13],
            ],
            face_set_index: vec![(0..12).collect()],
            name: String::from("D"),
        }
    }

    pub fn icosahedron() -> Self {
        let c0 = 0.809_017;

        Self {
            points: vec![
                Point::new(0.5, 0.0, c0),
                Point::new(0.5, 0.0, -c0),
                Point::new(-0.5, 0.0, c0),
                Point::new(-0.5, 0.0, -c0),
                Point::new(c0, 0.5, 0.0),
                Point::new(c0, -0.5, 0.0),
                Point::new(-c0, 0.5, 0.0),
                Point::new(-c0, -0.5, 0.0),
                Point::new(0.0, c0, 0.5),
                Point::new(0.0, c0, -0.5),
                Point::new(0.0, -c0, 0.5),
                Point::new(0.0, -c0, -0.5),
            ],
            face_index: vec![
                vec![10, 2, 0],
                vec![5, 10, 0],
                vec![4, 5, 0],
                vec![8, 4, 0],
                vec![2, 8, 0],
                vec![6, 8, 2],
                vec![7, 6, 2],
                vec![10, 7, 2],
                vec![11, 7, 10],
                vec![5, 11, 10],
                vec![1, 11, 5],
                vec![4, 1, 5],
                vec![9, 1, 4],
                vec![8, 9, 4],
                vec![6, 9, 8],
                vec![3, 9, 6],
                vec![7, 3, 6],
                vec![11, 3, 7],
                vec![1, 3, 11],
                vec![9, 3, 1],
            ],
            face_set_index: vec![(0..20).collect()],
            name: String::from("I"),
        }
    }
}
