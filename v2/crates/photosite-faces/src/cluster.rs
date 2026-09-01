//! Putting faces nobody has named into piles.
//!
//! The thresholds are the whole of this file's difficulty, and they are not
//! one number but three, because three different things are being decided:
//!
//! * **Grouping** an unnamed face with other unnamed faces costs nothing if
//!   it is wrong — somebody looks at the pile and sees a stranger in it. But
//!   a pile with two people in it gets named in one click and writes the
//!   wrong name into a file, so grouping still sits well above the model's
//!   own boundary. Splitting one person into two piles is the cheap mistake
//!   and is preferred.
//! * **Assigning** a face to a person who already has a name writes that
//!   name into a photograph with nobody watching, so it demands more.
//! * **Suggesting** is what the space between those two is for: probably
//!   them, not certainly, and the answer is a person's rather than ours.

use crate::math;

/// SFace's own decision boundary for "the same person", as published with
/// the model. Below this there is nothing to say.
pub const SAME_IDENTITY: f64 = 0.363;

/// How alike two unnamed faces must be to land in one pile.
pub const GROUPING: f64 = 0.45;

/// How alike a new face and a named person must be before the name is
/// written without asking.
pub const AUTO_ASSIGN: f64 = 0.5;

/// One pile of faces the arithmetic believes are one person.
#[derive(Debug, Clone, PartialEq)]
pub struct Cluster<T> {
    pub members: Vec<T>,
    pub centroid: Vec<f32>,
}

/// Greedy clustering around running centroids.
///
/// Every face joins the pile whose average it is most like, if that
/// likeness clears the threshold, and otherwise starts one of its own. The
/// **most confident detections go first**, so the piles are founded on the
/// sharpest, most frontal faces rather than on whichever happened to be read
/// out of the catalogue first — a seed is what everything after it is
/// measured against, and a blurred profile makes a poor one.
///
/// This is not the best clustering there is. It is the one whose mistakes
/// are the cheap kind: it splits before it merges.
///
/// `look` says how to see one item as a face — how sure the detector was,
/// and the vector. A closure rather than a trait, so that whatever holds the
/// faces need not know this crate exists: the catalogue's own face type is
/// clustered without either crate depending on the other.
pub fn cluster<T: Clone>(
    faces: &[T],
    threshold: f64,
    look: impl Fn(&T) -> (f64, &[f32]),
) -> Vec<Cluster<T>> {
    let mut order: Vec<&T> = faces.iter().collect();
    order.sort_by(|a, b| {
        look(b)
            .0
            .partial_cmp(&look(a).0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut clusters: Vec<Cluster<T>> = Vec::new();
    for face in order {
        let embedding = look(face).1;
        let mut best: Option<(usize, f64)> = None;
        for (index, cluster) in clusters.iter().enumerate() {
            let similarity = math::cosine(embedding, &cluster.centroid);
            if similarity >= threshold && best.is_none_or(|(_, so_far)| similarity > so_far) {
                best = Some((index, similarity));
            }
        }

        match best {
            Some((index, _)) => {
                clusters[index].members.push(face.clone());
                clusters[index].centroid = math::centroid(
                    &clusters[index]
                        .members
                        .iter()
                        .map(|member| look(member).1.to_vec())
                        .collect::<Vec<_>>(),
                );
            }
            None => clusters.push(Cluster {
                centroid: math::normalize(embedding),
                members: vec![face.clone()],
            }),
        }
    }

    // The biggest pile first: it is the one worth naming, and a person
    // opening the window should not have to hunt for it.
    clusters.sort_by_key(|cluster| std::cmp::Reverse(cluster.members.len()));
    clusters
}

/// What a freshly scanned face should be done with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Nobody it resembles. It waits in the unnamed pool.
    Unknown,
    /// Certainly this person; the name may be written.
    Assign(i64),
    /// Probably this person. It waits for a yes or a no and writes nothing
    /// in the meantime.
    Suggest(i64),
}

/// One named person as something to compare a new face against.
#[derive(Debug, Clone, PartialEq)]
pub struct Known {
    pub id: i64,
    pub centroid: Vec<f32>,
}

/// Which known person a face belongs to, if any.
///
/// The best match wins, and only if it clears the model's own boundary.
/// Between that boundary and [`AUTO_ASSIGN`] the answer is a suggestion:
/// good enough to be worth asking about, not good enough to write into
/// somebody's photograph unasked.
pub fn identify(embedding: &[f32], known: &[Known]) -> (Verdict, f64) {
    let mut best: Option<(&Known, f64)> = None;
    for person in known {
        let similarity = math::cosine(embedding, &person.centroid);
        if similarity >= SAME_IDENTITY && best.is_none_or(|(_, so_far)| similarity > so_far) {
            best = Some((person, similarity));
        }
    }

    match best {
        None => (Verdict::Unknown, 0.0),
        Some((person, similarity)) if similarity >= AUTO_ASSIGN => {
            (Verdict::Assign(person.id), similarity)
        }
        Some((person, similarity)) => (Verdict::Suggest(person.id), similarity),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    struct Sample {
        name: &'static str,
        confidence: f64,
        embedding: Vec<f32>,
    }

    /// How the clusterer is told to read a sample.
    fn look(sample: &Sample) -> (f64, &[f32]) {
        (sample.confidence, &sample.embedding)
    }

    /// A vector `spread` away from a direction, so two faces of one person
    /// can be built without a model.
    fn near(direction: usize, spread: f32, name: &'static str, confidence: f64) -> Sample {
        let mut embedding = vec![0f32; 8];
        embedding[direction] = 1.0;
        embedding[(direction + 1) % 8] = spread;
        Sample {
            name,
            confidence,
            embedding: math::normalize(&embedding),
        }
    }

    #[test]
    fn faces_of_one_person_end_up_in_one_pile() {
        let faces = vec![
            near(0, 0.1, "a", 0.9),
            near(0, 0.15, "b", 0.8),
            near(0, 0.05, "c", 0.7),
        ];
        let clusters = cluster(&faces, GROUPING, look);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].members.len(), 3);
    }

    #[test]
    fn two_people_do_not_end_up_in_one_pile() {
        let faces = vec![near(0, 0.1, "a", 0.9), near(4, 0.1, "b", 0.9)];
        let clusters = cluster(&faces, GROUPING, look);
        assert_eq!(clusters.len(), 2);
    }

    /// The seed decides what everything after it is compared with, so the
    /// sharpest face has to go first.
    #[test]
    fn the_most_confident_face_founds_the_pile() {
        let faces = vec![
            near(0, 0.9, "blurred", 0.3),
            near(0, 0.0, "sharp", 0.99),
            near(0, 0.1, "another", 0.5),
        ];
        let clusters = cluster(&faces, GROUPING, look);
        assert_eq!(clusters[0].members[0].name, "sharp");
    }

    #[test]
    fn the_biggest_pile_comes_first() {
        let mut faces = vec![near(4, 0.0, "alone", 0.99)];
        for index in 0..3 {
            faces.push(near(0, 0.05 * index as f32, "crowd", 0.5));
        }

        let clusters = cluster(&faces, GROUPING, look);
        assert_eq!(clusters[0].members.len(), 3);
        assert_eq!(clusters[1].members[0].name, "alone");
    }

    #[test]
    fn nothing_at_all_makes_no_piles() {
        let clusters = cluster::<Sample>(&[], GROUPING, look);
        assert!(clusters.is_empty());
    }

    #[test]
    fn a_face_that_matches_nobody_stays_unknown() {
        let known = vec![Known {
            id: 1,
            centroid: near(0, 0.0, "x", 1.0).embedding,
        }];
        let stranger = near(4, 0.0, "y", 1.0).embedding;
        assert_eq!(identify(&stranger, &known).0, Verdict::Unknown);
    }

    #[test]
    fn a_certain_match_is_assigned() {
        let person = near(0, 0.0, "x", 1.0).embedding;
        let known = vec![Known {
            id: 7,
            centroid: person.clone(),
        }];
        assert_eq!(identify(&person, &known).0, Verdict::Assign(7));
    }

    /// The band between the model's boundary and the auto-assign threshold
    /// is the whole reason suggestions exist. If it were empty, every
    /// borderline face would either be written into a file unasked or
    /// silently dropped.
    #[test]
    fn a_borderline_match_is_asked_about_rather_than_written() {
        let mut probably = vec![0f32; 8];
        probably[0] = 1.0;
        // cosine with (1,0,..) is 1/sqrt(1+k*k), which for k = 2.16 lands
        // at 0.42 — between the model's boundary and the auto-assign line.
        probably[1] = 2.16;
        let probably = math::normalize(&probably);

        let mut centroid = vec![0f32; 8];
        centroid[0] = 1.0;
        let known = vec![Known {
            id: 3,
            centroid: math::normalize(&centroid),
        }];

        let (verdict, similarity) = identify(&probably, &known);
        assert!(
            (SAME_IDENTITY..AUTO_ASSIGN).contains(&similarity),
            "the test's own vector is not in the band: {similarity}"
        );
        assert_eq!(verdict, Verdict::Suggest(3));
    }

    #[test]
    fn the_best_of_several_people_wins() {
        let mut close = vec![0f32; 8];
        close[0] = 1.0;
        close[1] = 0.2;
        let known = vec![
            Known {
                id: 1,
                centroid: math::normalize(&close),
            },
            Known {
                id: 2,
                centroid: near(0, 0.0, "x", 1.0).embedding,
            },
        ];
        let mut face = vec![0f32; 8];
        face[0] = 1.0;
        assert_eq!(
            identify(&math::normalize(&face), &known).0,
            Verdict::Assign(2)
        );
    }

    /// The three thresholds have to stay in order or the band disappears and
    /// suggestions stop happening at all, silently.
    #[test]
    fn the_thresholds_stay_in_order() {
        const { assert!(SAME_IDENTITY < GROUPING) };
        const { assert!(GROUPING < AUTO_ASSIGN) };
    }
}
