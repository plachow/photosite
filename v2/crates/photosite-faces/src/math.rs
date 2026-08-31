//! The arithmetic of an embedding.
//!
//! Two vectors and one number between them: that number is the whole of face
//! recognition here, and everything else in the crate exists to produce the
//! vectors honestly.

/// The length of an SFace embedding. Not used to allocate — the model says
/// how long its own output is — but to refuse a vector that plainly came
/// from somewhere else.
pub const DIMENSIONS: usize = 128;

/// Scales a vector to unit length.
///
/// Every embedding is stored normalised, which makes [`cosine`] a dot
/// product and makes averaging a set of them meaningful. A vector of
/// practically zero length is left alone rather than divided by nothing.
pub fn normalize(vector: &[f32]) -> Vec<f32> {
    let length = vector
        .iter()
        .map(|value| f64::from(*value) * f64::from(*value))
        .sum::<f64>()
        .sqrt();
    if length < 1e-12 {
        return vector.to_vec();
    }

    vector
        .iter()
        .map(|value| (f64::from(*value) / length) as f32)
        .collect()
}

/// Cosine similarity: 1 is the same face, 0 is unrelated.
///
/// It normalises as it goes rather than trusting that both sides were
/// normalised. A centroid built from three vectors is not unit length until
/// somebody makes it so, and a similarity that quietly depends on whether
/// anybody remembered is the kind of bug that shows up as "recognition got
/// worse" months later.
pub fn cosine(first: &[f32], second: &[f32]) -> f64 {
    if first.len() != second.len() || first.is_empty() {
        return 0.0;
    }

    let mut dot = 0f64;
    let mut left = 0f64;
    let mut right = 0f64;
    for (a, b) in first.iter().zip(second) {
        dot += f64::from(*a) * f64::from(*b);
        left += f64::from(*a) * f64::from(*a);
        right += f64::from(*b) * f64::from(*b);
    }

    let scale = left.sqrt() * right.sqrt();
    if scale < 1e-12 { 0.0 } else { dot / scale }
}

/// The average of several embeddings, normalised — one person's face as a
/// single vector.
///
/// Averaging first and normalising afterwards is deliberate: the mean of
/// unit vectors is shorter than one, and its length carries how much the
/// members agree. Normalising each member first would throw that away, and
/// normalising nothing at all would let a person with forty faces outweigh
/// the comparison.
pub fn centroid(embeddings: &[Vec<f32>]) -> Vec<f32> {
    let Some(first) = embeddings.first() else {
        return Vec::new();
    };

    let mut sum = vec![0f64; first.len()];
    for embedding in embeddings {
        if embedding.len() != sum.len() {
            // A vector from another model is not comparable with these and
            // averaging it in would poison the person quietly.
            continue;
        }

        for (total, value) in sum.iter_mut().zip(embedding) {
            *total += f64::from(*value);
        }
    }

    let raw: Vec<f32> = sum.into_iter().map(|value| value as f32).collect();
    normalize(&raw)
}

/// A vector as it is stored in the catalogue, and back.
///
/// Little-endian on every platform, so a catalogue copied from one machine
/// to another reads the same. The native byte order would work everywhere it
/// was written and nowhere else.
pub fn to_blob(embedding: &[f32]) -> Vec<u8> {
    embedding
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

pub fn from_blob(blob: &[u8]) -> Vec<f32> {
    blob.as_chunks::<4>()
        .0
        .iter()
        .map(|bytes| f32::from_le_bytes(*bytes))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_normalised_vector_is_one_long() {
        let unit = normalize(&[3.0, 4.0]);
        let length: f32 = unit.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((length - 1.0).abs() < 1e-6, "{unit:?}");
    }

    #[test]
    fn nothing_is_divided_by_nothing() {
        assert_eq!(normalize(&[0.0, 0.0]), vec![0.0, 0.0]);
    }

    #[test]
    fn the_same_vector_is_the_same_face() {
        let vector = vec![0.1, -0.5, 0.3, 0.8];
        assert!((cosine(&vector, &vector) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn an_opposite_vector_is_not_the_same_face() {
        let vector = vec![0.1, -0.5, 0.3];
        let opposite: Vec<f32> = vector.iter().map(|v| -v).collect();
        assert!(cosine(&vector, &opposite) < -0.99);
    }

    #[test]
    fn vectors_of_different_lengths_are_not_compared() {
        assert_eq!(cosine(&[1.0, 0.0], &[1.0, 0.0, 0.0]), 0.0);
        assert_eq!(cosine(&[], &[]), 0.0);
    }

    /// The centroid has to sit between its members. Without that a person
    /// with two very different photographs would match neither.
    #[test]
    fn a_centroid_sits_between_the_faces_it_came_from() {
        let a = normalize(&[1.0, 0.0]);
        let b = normalize(&[0.0, 1.0]);
        let middle = centroid(&[a.clone(), b.clone()]);
        let to_a = cosine(&middle, &a);
        let to_b = cosine(&middle, &b);
        assert!((to_a - to_b).abs() < 1e-6, "{to_a} vs {to_b}");
        assert!(to_a > 0.7, "{to_a}");
    }

    #[test]
    fn a_vector_from_another_model_is_left_out_of_the_average() {
        let mixed = centroid(&[vec![1.0, 0.0], vec![0.0, 1.0, 0.0]]);
        assert_eq!(mixed.len(), 2);
        assert!((cosine(&mixed, &[1.0, 0.0]) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn an_embedding_survives_the_trip_through_the_catalogue() {
        let embedding: Vec<f32> = (0..DIMENSIONS).map(|i| i as f32 / 128.0 - 0.5).collect();
        assert_eq!(from_blob(&to_blob(&embedding)), embedding);
    }

    #[test]
    fn a_truncated_blob_gives_up_the_tail_rather_than_panicking() {
        let blob = to_blob(&[1.0, 2.0, 3.0]);
        assert_eq!(from_blob(&blob[..9]), vec![1.0, 2.0]);
    }
}
