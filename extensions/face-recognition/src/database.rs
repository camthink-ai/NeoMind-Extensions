//! Face database module for managing registered faces with feature vectors.
//!
//! Provides CRUD operations for face entries, cosine similarity matching,
//! and JSON-based persistence with secure file permissions.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{BufReader, BufWriter};
use std::path::Path;
use thiserror::Error;

// ============================================================================
// Error Types
// ============================================================================

/// Errors produced by face database operations.
#[derive(Debug, Error)]
pub enum DatabaseError {
    #[error("face with name '{0}' already exists")]
    DuplicateName(String),

    #[error("maximum face limit of {0} reached")]
    MaxFacesReached(usize),

    #[error("face with id '{0}' not found")]
    NotFound(String),

    #[error("invalid feature vector: expected length {expected}, got {actual}")]
    InvalidFeature { expected: usize, actual: usize },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

pub type DatabaseResult<T> = Result<T, DatabaseError>;

// ============================================================================
// Data Types
// ============================================================================

/// A registered face entry with its feature vector and metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaceEntry {
    pub id: String,
    pub name: String,
    pub feature: Vec<f32>,
    pub thumbnail: String,
    pub registered_at: i64,
}

/// Summary of a face entry without the feature vector.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaceEntrySummary {
    pub id: String,
    pub name: String,
    pub registered_at: i64,
    pub thumbnail: String,
}

impl From<&FaceEntry> for FaceEntrySummary {
    fn from(entry: &FaceEntry) -> Self {
        Self {
            id: entry.id.clone(),
            name: entry.name.clone(),
            registered_at: entry.registered_at,
            thumbnail: entry.thumbnail.clone(),
        }
    }
}

/// Result of matching a face feature against the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchResult {
    pub face_id: String,
    pub name: String,
    pub similarity: f64,
}

/// Serializable container for persisting the full database state.
#[derive(Debug, Serialize, Deserialize)]
struct FaceDatabaseFile {
    threshold: f64,
    max_faces: usize,
    faces: Vec<FaceEntry>,
}

// ============================================================================
// Face Database
// ============================================================================

/// In-memory face database with cosine similarity matching and JSON persistence.
#[derive(Debug)]
pub struct FaceDatabase {
    /// Cosine similarity threshold for positive matches.
    threshold: f64,
    /// Maximum number of faces allowed in the database.
    max_faces: usize,
    /// Face entries indexed by face id.
    faces_by_id: HashMap<String, FaceEntry>,
    /// Name-to-id index for duplicate detection.
    id_by_name: HashMap<String, String>,
}

impl FaceDatabase {
    /// Create a new empty face database.
    ///
    /// # Arguments
    /// * `threshold` - Minimum cosine similarity for a match (0.0 to 1.0).
    /// * `max_faces` - Maximum number of faces the database can hold.
    pub fn new(threshold: f64, max_faces: usize) -> Self {
        Self {
            threshold,
            max_faces,
            faces_by_id: HashMap::new(),
            id_by_name: HashMap::new(),
        }
    }

    /// Register a new face in the database.
    ///
    /// Generates a unique ID and stores the feature vector along with metadata.
    /// Returns an error if the name is already taken or the database is full.
    pub fn register(
        &mut self,
        name: &str,
        feature: Vec<f32>,
        thumbnail: &str,
    ) -> DatabaseResult<FaceEntry> {
        // Reject duplicate names
        if self.id_by_name.contains_key(name) {
            return Err(DatabaseError::DuplicateName(name.to_string()));
        }

        // Enforce maximum faces limit
        if self.faces_by_id.len() >= self.max_faces {
            return Err(DatabaseError::MaxFacesReached(self.max_faces));
        }

        let entry = FaceEntry {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            feature,
            thumbnail: thumbnail.to_string(),
            registered_at: chrono::Utc::now().timestamp(),
        };

        self.id_by_name.insert(entry.name.clone(), entry.id.clone());
        self.faces_by_id.insert(entry.id.clone(), entry.clone());

        Ok(entry)
    }

    /// Delete a face entry by its ID.
    pub fn delete(&mut self, face_id: &str) -> DatabaseResult<()> {
        let entry = self
            .faces_by_id
            .remove(face_id)
            .ok_or_else(|| DatabaseError::NotFound(face_id.to_string()))?;

        self.id_by_name.remove(&entry.name);
        Ok(())
    }

    /// Match a feature vector against all registered faces.
    ///
    /// Returns the best match above the similarity threshold, or `None` if
    /// no face exceeds the threshold.
    pub fn match_face(&self, feature: &[f32]) -> Option<MatchResult> {
        let mut best: Option<MatchResult> = None;

        for entry in self.faces_by_id.values() {
            let sim = cosine_similarity(feature, &entry.feature);
            if sim >= self.threshold {
                match &best {
                    Some(current) if sim <= current.similarity => {}
                    _ => {
                        best = Some(MatchResult {
                            face_id: entry.id.clone(),
                            name: entry.name.clone(),
                            similarity: sim,
                        });
                    }
                }
            }
        }

        best
    }

    /// List all registered faces as summaries (without feature vectors).
    pub fn list_faces(&self) -> Vec<FaceEntrySummary> {
        self.faces_by_id
            .values()
            .map(FaceEntrySummary::from)
            .collect()
    }

    /// Get the number of registered faces.
    pub fn len(&self) -> usize {
        self.faces_by_id.len()
    }

    /// Check if the database is empty.
    pub fn is_empty(&self) -> bool {
        self.faces_by_id.is_empty()
    }

    /// Update the cosine similarity threshold for matching.
    ///
    /// This allows runtime reconfiguration of the matching threshold
    /// without rebuilding the database.
    pub fn set_threshold(&mut self, threshold: f64) {
        self.threshold = threshold;
    }

    /// Update the maximum number of faces allowed.
    pub fn set_max_faces(&mut self, max_faces: usize) {
        self.max_faces = max_faces;
    }

    /// Get a face entry by ID.
    pub fn get(&self, face_id: &str) -> Option<&FaceEntry> {
        self.faces_by_id.get(face_id)
    }

    /// Save the database to a JSON file with restricted permissions (0600 on Unix).
    pub fn save_to_file(&self, path: &Path) -> DatabaseResult<()> {
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;

        // Set file permissions to 0600 (owner read/write only) on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            file.set_permissions(perms)?;
        }

        let writer = BufWriter::new(file);
        let data = FaceDatabaseFile {
            threshold: self.threshold,
            max_faces: self.max_faces,
            faces: self.faces_by_id.values().cloned().collect(),
        };
        serde_json::to_writer_pretty(writer, &data)?;

        Ok(())
    }

    /// Load a database from a JSON file, restoring all face entries.
    pub fn load_from_file(path: &Path) -> DatabaseResult<Self> {
        let file = OpenOptions::new().read(true).open(path)?;
        let reader = BufReader::new(file);
        let data: FaceDatabaseFile = serde_json::from_reader(reader)?;

        let mut faces_by_id = HashMap::new();
        let mut id_by_name = HashMap::new();

        for entry in data.faces {
            id_by_name.insert(entry.name.clone(), entry.id.clone());
            faces_by_id.insert(entry.id.clone(), entry);
        }

        Ok(Self {
            threshold: data.threshold,
            max_faces: data.max_faces,
            faces_by_id,
            id_by_name,
        })
    }
}

// ============================================================================
// Cosine Similarity
// ============================================================================

/// Compute cosine similarity between two feature vectors.
///
/// Returns a value in the range [-1.0, 1.0], where 1.0 means identical
/// direction. Returns 0.0 if either vector has zero magnitude.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let mut dot_product: f64 = 0.0;
    let mut norm_a: f64 = 0.0;
    let mut norm_b: f64 = 0.0;

    for (va, vb) in a.iter().zip(b.iter()) {
        dot_product += f64::from(*va) * f64::from(*vb);
        norm_a += f64::from(*va) * f64::from(*va);
        norm_b += f64::from(*vb) * f64::from(*vb);
    }

    let denominator = norm_a.sqrt() * norm_b.sqrt();
    if denominator == 0.0 {
        return 0.0;
    }

    dot_product / denominator
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Helper: create a normalized feature vector of the given dimension.
    fn make_feature(dim: usize, seed: f32) -> Vec<f32> {
        let mut v: Vec<f32> = (0..dim).map(|i| seed + i as f32 * 0.01).collect();
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            v.iter_mut().for_each(|x| *x /= norm);
        }
        v
    }

    // ------------------------------------------------------------------
    // Register
    // ------------------------------------------------------------------

    #[test]
    fn register_adds_face_and_returns_entry() {
        let mut db = FaceDatabase::new(0.45, 10);
        let feature = make_feature(128, 1.0);

        let entry = db.register("Alice", feature.clone(), "thumb_alice").unwrap();

        assert!(!entry.id.is_empty());
        assert_eq!(entry.name, "Alice");
        assert_eq!(entry.feature, feature);
        assert_eq!(entry.thumbnail, "thumb_alice");
        assert!(entry.registered_at > 0);
        assert_eq!(db.len(), 1);
    }

    // ------------------------------------------------------------------
    // Delete
    // ------------------------------------------------------------------

    #[test]
    fn delete_removes_face() {
        let mut db = FaceDatabase::new(0.45, 10);
        let entry = db.register("Bob", make_feature(128, 2.0), "thumb_bob").unwrap();

        assert!(db.delete(&entry.id).is_ok());
        assert_eq!(db.len(), 0);
        assert!(db.get(&entry.id).is_none());
    }

    #[test]
    fn delete_nonexistent_returns_not_found() {
        let mut db = FaceDatabase::new(0.45, 10);
        let err = db.delete("no-such-id").unwrap_err();
        assert!(matches!(err, DatabaseError::NotFound(_)));
    }

    // ------------------------------------------------------------------
    // Duplicate name rejection
    // ------------------------------------------------------------------

    #[test]
    fn register_rejects_duplicate_name() {
        let mut db = FaceDatabase::new(0.45, 10);
        db.register("Carol", make_feature(128, 3.0), "thumb").unwrap();

        let err = db.register("Carol", make_feature(128, 4.0), "thumb2").unwrap_err();
        assert!(matches!(err, DatabaseError::DuplicateName(name) if name == "Carol"));
    }

    // ------------------------------------------------------------------
    // Max faces limit
    // ------------------------------------------------------------------

    #[test]
    fn register_enforces_max_faces_limit() {
        let mut db = FaceDatabase::new(0.45, 2);
        db.register("A", make_feature(128, 1.0), "t").unwrap();
        db.register("B", make_feature(128, 2.0), "t").unwrap();

        let err = db.register("C", make_feature(128, 3.0), "t").unwrap_err();
        assert!(matches!(err, DatabaseError::MaxFacesReached(2)));
    }

    // ------------------------------------------------------------------
    // Match face
    // ------------------------------------------------------------------

    #[test]
    fn match_face_returns_best_above_threshold() {
        let mut db = FaceDatabase::new(0.5, 10);

        // Register two faces with distinct feature vectors
        let feat_dave = make_feature(128, 1.0);
        let feat_eve = make_feature(128, 10.0);
        db.register("Dave", feat_dave.clone(), "thumb_dave").unwrap();
        db.register("Eve", feat_eve, "thumb_eve").unwrap();

        // Query with Dave's exact feature -> should match Dave with similarity 1.0
        let result = db.match_face(&feat_dave).unwrap();
        assert_eq!(result.name, "Dave");
        assert!((result.similarity - 1.0).abs() < 1e-6);
    }

    #[test]
    fn match_face_returns_none_below_threshold() {
        let mut db = FaceDatabase::new(0.5, 10);

        // Create two truly different features: one active in first half, one in second half
        let mut feat_registered = vec![0.0_f32; 128];
        let mut feat_query = vec![0.0_f32; 128];
        for i in 0..64 {
            feat_registered[i] = 1.0;
            feat_query[64 + i] = 1.0;
        }
        // Normalize both
        let norm_r: f32 = feat_registered.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_q: f32 = feat_query.iter().map(|x| x * x).sum::<f32>().sqrt();
        feat_registered.iter_mut().for_each(|x| *x /= norm_r);
        feat_query.iter_mut().for_each(|x| *x /= norm_q);

        db.register("Frank", feat_registered, "thumb").unwrap();

        // Orthogonal vectors have cosine similarity = 0, well below 0.5 threshold
        assert!(db.match_face(&feat_query).is_none());
    }

    #[test]
    fn match_face_returns_none_on_empty_database() {
        let db = FaceDatabase::new(0.45, 10);
        assert!(db.match_face(&make_feature(128, 1.0)).is_none());
    }

    // ------------------------------------------------------------------
    // List faces
    // ------------------------------------------------------------------

    #[test]
    fn list_faces_returns_summaries_without_features() {
        let mut db = FaceDatabase::new(0.45, 10);
        let e1 = db.register("G1", make_feature(128, 1.0), "t1").unwrap();
        let e2 = db.register("G2", make_feature(128, 2.0), "t2").unwrap();

        let summaries = db.list_faces();
        assert_eq!(summaries.len(), 2);

        let ids: Vec<&str> = summaries.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains(&e1.id.as_str()));
        assert!(ids.contains(&e2.id.as_str()));

        // Verify no feature vectors in summaries (they would be large)
        let serialized = serde_json::to_string(&summaries).unwrap();
        assert!(!serialized.contains("feature"));
    }

    // ------------------------------------------------------------------
    // Save / Load round-trip
    // ------------------------------------------------------------------

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("faces.json");

        let mut db = FaceDatabase::new(0.45, 10);
        let e1 = db.register("Hank", make_feature(128, 1.0), "thumb_hank").unwrap();
        let e2 = db.register("Ivy", make_feature(128, 2.0), "thumb_ivy").unwrap();

        db.save_to_file(&path).unwrap();

        // Verify file exists and is valid JSON
        assert!(path.exists());
        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.contains("Hank"));
        assert!(raw.contains("Ivy"));

        // Load into a new database
        let loaded = FaceDatabase::load_from_file(&path).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded.threshold, 0.45);
        assert_eq!(loaded.max_faces, 10);

        // Verify the feature vectors survived the round trip
        let loaded_e1 = loaded.get(&e1.id).unwrap();
        assert_eq!(loaded_e1.feature, e1.feature);
        assert_eq!(loaded_e1.name, "Hank");

        let loaded_e2 = loaded.get(&e2.id).unwrap();
        assert_eq!(loaded_e2.feature, e2.feature);
        assert_eq!(loaded_e2.name, "Ivy");

        // Verify matching still works after reload
        let result = loaded.match_face(&e1.feature).unwrap();
        assert_eq!(result.name, "Hank");
    }

    #[test]
    fn load_nonexistent_file_returns_io_error() {
        let result = FaceDatabase::load_from_file(Path::new("/tmp/face_rec_nonexistent_12345.json"));
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), DatabaseError::Io(_)));
    }

    // ------------------------------------------------------------------
    // Delete allows re-register with same name
    // ------------------------------------------------------------------

    #[test]
    fn delete_allows_reregister_with_same_name() {
        let mut db = FaceDatabase::new(0.45, 10);
        let entry = db.register("Jack", make_feature(128, 1.0), "t1").unwrap();
        db.delete(&entry.id).unwrap();

        // Should succeed since the name is now free
        let new_entry = db.register("Jack", make_feature(128, 2.0), "t2").unwrap();
        assert_eq!(new_entry.name, "Jack");
        assert_ne!(new_entry.id, entry.id);
    }

    // ------------------------------------------------------------------
    // Cosine similarity unit tests
    // ------------------------------------------------------------------

    #[test]
    fn cosine_similarity_identical_vectors() {
        let v = vec![1.0_f32, 2.0, 3.0];
        let sim = cosine_similarity(&v, &v);
        assert!((sim - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_similarity_orthogonal_vectors() {
        let a = vec![1.0_f32, 0.0];
        let b = vec![0.0_f32, 1.0];
        let sim = cosine_similarity(&a, &b);
        assert!(sim.abs() < 1e-6);
    }

    #[test]
    fn cosine_similarity_opposite_vectors() {
        let a = vec![1.0_f32, 0.0];
        let b = vec![-1.0_f32, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim + 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_similarity_zero_vector() {
        let a = vec![0.0_f32, 0.0];
        let b = vec![1.0_f32, 2.0];
        let sim = cosine_similarity(&a, &b);
        assert!(sim.abs() < 1e-6);
    }

    #[test]
    fn cosine_similarity_mismatched_lengths() {
        let a = vec![1.0_f32];
        let b = vec![1.0_f32, 2.0];
        let sim = cosine_similarity(&a, &b);
        assert_eq!(sim, 0.0);
    }
}
