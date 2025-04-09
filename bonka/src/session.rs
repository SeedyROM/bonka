use dashmap::DashMap;
use rand::{TryRngCore, rngs::OsRng};
use sha3::{Digest, Sha3_256};
use std::time::Instant;

use crate::{constants::DEFAULT_APP_SECRET, log};

/// Session ID is a unique identifier for each session.
pub type Id = u64;

/// Session struct represents a user session.
pub struct Session {
    /// Unique session ID generated based on the origin and app secret.
    pub id: Id,
    /// Timestamp of when the session started.
    pub start_time: Instant,
    /// Timestamp of the last activity in the session.
    pub last_activity: Instant,
    /// Origin of the session, typically a URL or identifier for the client.
    pub origin: String,
}

impl Session {
    /// Creates a new session with the given origin and an optional custom seed.
    pub fn new(origin: impl Into<String>, custom_seed: Option<&[u8]>) -> Self {
        let origin = origin.into();

        let mut hasher = Sha3_256::new();
        hasher.update(origin.as_bytes());

        let app_secret = match std::env::var("BONKA_APP_SECRET") {
            Ok(secret) => secret.into_bytes().to_vec(),
            Err(_) => {
                log::warn!(
                    "BONKA_APP_SECRET environment variable not set. Using default application secret."
                );
                DEFAULT_APP_SECRET.to_vec()
            }
        };
        hasher.update(app_secret);

        // Add optional custom seed if provided (for reproducibility)
        if let Some(seed) = custom_seed {
            hasher.update(seed);
        }

        // Finalize and get first 8 bytes as u64
        let result = hasher.finalize();
        let mut id_bytes = [0u8; 8];
        id_bytes.copy_from_slice(&result[0..8]);
        let id = Id::from_le_bytes(id_bytes);

        Session {
            id,
            start_time: Instant::now(),
            last_activity: Instant::now(),
            origin,
        }
    }

    /// Creates a new session with the given origin using default application secret.
    pub fn new_default(origin: impl Into<String>) -> Self {
        Self::new(origin, None)
    }

    /// Update the last activity timestamp to the current time.
    pub fn touch(&mut self) {
        self.last_activity = Instant::now();
    }
}

pub struct SessionManager {
    sessions: DashMap<Id, Session>,
    origin_to_sessions: DashMap<String, Vec<Id>>,
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionManager {
    pub fn new() -> Self {
        SessionManager {
            sessions: DashMap::new(),
            origin_to_sessions: DashMap::new(),
        }
    }

    pub fn create_session(&self, origin: impl Into<String>) -> Session {
        let origin_str = origin.into();
        // Generate random bytes to ensure uniqueness
        let mut random_bytes = [0u8; 16];
        OsRng
            .try_fill_bytes(&mut random_bytes)
            .expect("Failed to generate random bytes for session");

        let session = Session::new(origin_str.clone(), Some(&random_bytes));
        let id = session.id;

        // Store the session
        self.sessions.insert(id, session.clone());

        // Update the origin-to-sessions mapping
        self.origin_to_sessions
            .entry(origin_str)
            .or_default()
            .push(id);

        session
    }

    // Get all sessions for a specific origin
    pub fn get_sessions_by_origin(&self, origin: &str) -> Vec<Session> {
        if let Some(ids) = self.origin_to_sessions.get(origin) {
            ids.iter()
                .filter_map(|id| self.sessions.get(id).map(|session| session.clone()))
                .collect()
        } else {
            Vec::new()
        }
    }

    // Remove a session and update origin mapping
    pub fn remove_session(&self, id: Id) -> bool {
        if let Some(session) = self.sessions.remove(&id) {
            // Update the origin mapping
            if let Some(mut ids) = self.origin_to_sessions.get_mut(&session.1.origin) {
                ids.retain(|&sid| sid != id);
                if ids.is_empty() {
                    // Need to drop the reference before removing
                    drop(ids);
                    self.origin_to_sessions.remove(&session.1.origin);
                }
            }
            true
        } else {
            false
        }
    }

    /// Creates a new session with the given origin and custom seed
    pub fn create_session_with_seed(&self, origin: impl Into<String>, seed: &[u8]) -> Session {
        let session = Session::new(origin.into(), Some(seed));
        let id = session.id;
        self.sessions.insert(id, session.clone());
        session
    }

    /// Gets a reference to a session by its ID
    pub fn get_session(&self, id: Id) -> Option<Session> {
        self.sessions.get(&id).map(|session| session.clone())
    }

    /// Updates the last activity timestamp of a session
    pub fn touch_session(&self, id: Id) -> bool {
        if let Some(mut session) = self.sessions.get_mut(&id) {
            session.touch();
            true
        } else {
            false
        }
    }

    /// Gets all sessions
    pub fn get_all_sessions(&self) -> Vec<Session> {
        self.sessions
            .iter()
            .map(|session| session.clone())
            .collect()
    }

    /// Removes all sessions
    pub fn clear_sessions(&self) {
        self.sessions.clear();
        self.origin_to_sessions.clear();
    }

    /// Gets the number of active sessions
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    /// Finds sessions by origin
    pub fn find_sessions_by_origin(&self, origin: &str) -> Vec<Session> {
        self.sessions
            .iter()
            .filter(|session| session.origin == origin)
            .map(|session| session.clone())
            .collect()
    }

    /// Removes sessions that have been inactive for longer than the given duration
    pub fn cleanup_inactive_sessions(&self, timeout: std::time::Duration) {
        let now = Instant::now();
        let ids_to_remove: Vec<Id> = self
            .sessions
            .iter()
            .filter(|session| now.duration_since(session.last_activity) >= timeout)
            .map(|session| session.id)
            .collect();

        // Remove the inactive sessions
        for id in ids_to_remove {
            self.remove_session(id);
        }
    }
}

// Need to implement Clone for Session to work with DashMap
impl Clone for Session {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            start_time: self.start_time,
            last_activity: self.last_activity,
            origin: self.origin.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn create_session() {
        let manager = SessionManager::new();
        let session = manager.create_session("127.0.0.1:8080");

        assert_eq!(session.origin, "127.0.0.1:8080");
        assert_eq!(manager.session_count(), 1);
    }

    #[test]
    fn multiple_sessions_same_origin() {
        let manager = SessionManager::new();

        // Create two sessions with the same IP:port origin
        let session1 = manager.create_session("192.168.1.1:8080");
        let id1 = session1.id;

        let session2 = manager.create_session("192.168.1.1:8080");
        let id2 = session2.id;

        // Verify they have different IDs
        assert_ne!(id1, id2);

        // Verify both sessions exist in the manager
        assert_eq!(manager.session_count(), 2);

        // Verify get_sessions_by_origin returns both sessions
        let origin_sessions = manager.get_sessions_by_origin("192.168.1.1:8080");
        assert_eq!(origin_sessions.len(), 2);

        // Verify the origins are preserved correctly
        assert!(
            origin_sessions
                .iter()
                .all(|s| s.origin == "192.168.1.1:8080")
        );
    }

    #[test]
    fn get_session() {
        let manager = SessionManager::new();
        let session = manager.create_session("10.0.0.1:9000");
        let id = session.id;

        // Verify we can retrieve the session
        let retrieved = manager.get_session(id);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id, id);

        // Verify we get None for a non-existent session
        let non_existent = manager.get_session(12345);
        assert!(non_existent.is_none());
    }

    #[test]
    fn touch_session() {
        let manager = SessionManager::new();
        let session = manager.create_session("172.16.0.5:4433");
        let id = session.id;

        // Store the initial last_activity time
        let initial_time = session.last_activity;

        // Wait a moment to ensure time difference
        sleep(Duration::from_millis(10));

        // Touch the session
        let touch_result = manager.touch_session(id);
        assert!(touch_result);

        // Verify the last_activity was updated
        let updated = manager.get_session(id).unwrap();
        assert!(updated.last_activity > initial_time);

        // Verify touching a non-existent session returns false
        assert!(!manager.touch_session(12345));
    }

    #[test]
    fn remove_session() {
        let manager = SessionManager::new();
        let session = manager.create_session("192.168.0.1:22");
        let id = session.id;

        // Verify the session exists
        assert_eq!(manager.session_count(), 1);

        // Remove the session
        let remove_result = manager.remove_session(id);
        assert!(remove_result);

        // Verify the session was removed
        assert_eq!(manager.session_count(), 0);
        assert!(manager.get_session(id).is_none());

        // Verify the origin mapping was updated
        assert!(manager.get_sessions_by_origin("192.168.0.1:22").is_empty());

        // Verify removing a non-existent session returns false
        assert!(!manager.remove_session(id));
    }

    #[test]
    fn cleanup_inactive_sessions() {
        let manager = SessionManager::new();

        // Create a session
        let session = manager.create_session("10.10.10.10:80");
        let id = session.id;

        // Wait a moment
        sleep(Duration::from_millis(50));

        // Create another session and touch it
        let active_session = manager.create_session("10.10.10.11:80");
        let active_id = active_session.id;

        // Clean up sessions inactive for more than 25ms
        manager.cleanup_inactive_sessions(Duration::from_millis(25));

        // Verify the first session was removed but the second remains
        assert!(manager.get_session(id).is_none());
        assert!(manager.get_session(active_id).is_some());
        assert_eq!(manager.session_count(), 1);
    }

    #[test]
    fn clear_sessions() {
        let manager = SessionManager::new();

        // Create multiple sessions with different IP:port origins
        manager.create_session("192.168.1.10:5555");
        manager.create_session("192.168.1.11:5555");
        manager.create_session("192.168.1.12:5555");

        assert_eq!(manager.session_count(), 3);

        // Clear all sessions
        manager.clear_sessions();

        // Verify all sessions were removed
        assert_eq!(manager.session_count(), 0);
        assert!(
            manager
                .get_sessions_by_origin("192.168.1.10:5555")
                .is_empty()
        );
    }

    #[test]
    fn custom_seed() {
        let manager = SessionManager::new();

        // Create two sessions with the same origin but different seeds
        let seed1 = b"seed1";
        let seed2 = b"seed2";

        let session1 = manager.create_session_with_seed("192.168.5.5:1234", seed1);
        let id1 = session1.id;

        let session2 = manager.create_session_with_seed("192.168.5.5:1234", seed2);
        let id2 = session2.id;

        // Verify they have different IDs
        assert_ne!(id1, id2);

        // Create another session with the first seed - should match the first ID
        let session3 = manager.create_session_with_seed("192.168.5.5:1234", seed1);
        let id3 = session3.id;

        // Verify it has the same ID as the first session with the same seed
        assert_eq!(id1, id3);
    }

    #[test]
    fn find_sessions_by_origin() {
        let manager = SessionManager::new();

        // Add several sessions with different origins
        manager.create_session("192.168.1.1:8080");
        manager.create_session("192.168.1.2:8080");
        manager.create_session("192.168.1.1:8080"); // Duplicate origin
        manager.create_session("192.168.1.1:9090"); // Same IP, different port

        // Find sessions by exact origin match
        let sessions_8080 = manager.find_sessions_by_origin("192.168.1.1:8080");
        assert_eq!(sessions_8080.len(), 2);
        for session in &sessions_8080 {
            assert_eq!(session.origin, "192.168.1.1:8080");
        }

        // Find sessions for a different origin
        let sessions_9090 = manager.find_sessions_by_origin("192.168.1.1:9090");
        assert_eq!(sessions_9090.len(), 1);
        assert_eq!(sessions_9090[0].origin, "192.168.1.1:9090");

        // Find sessions for non-existent origin
        let non_existent = manager.find_sessions_by_origin("192.168.1.3:8080");
        assert_eq!(non_existent.len(), 0);
    }

    #[test]
    fn ipv6_address() {
        let manager = SessionManager::new();

        // Test with IPv6 address format
        let ipv6_origin = "[2001:db8::1]:8080";
        let session = manager.create_session(ipv6_origin);

        assert_eq!(session.origin, ipv6_origin);
        assert_eq!(manager.get_sessions_by_origin(ipv6_origin).len(), 1);
    }

    #[test]
    fn get_all_sessions() {
        let manager = SessionManager::new();

        // Initially there should be no sessions
        assert_eq!(manager.get_all_sessions().len(), 0);

        // Add three sessions
        let session1 = manager.create_session("192.168.1.1:8080");
        let id1 = session1.id;

        let session2 = manager.create_session("192.168.1.2:8080");
        let id2 = session2.id;

        let session3 = manager.create_session("192.168.1.3:8080");
        let id3 = session3.id;

        // Collect all sessions
        let all_sessions: Vec<_> = manager.get_all_sessions();

        // Verify count
        assert_eq!(all_sessions.len(), 3);

        // Verify all sessions are included
        let session_ids: HashSet<_> = all_sessions.iter().map(|s| s.id).collect();
        assert!(session_ids.contains(&id1));
        assert!(session_ids.contains(&id2));
        assert!(session_ids.contains(&id3));
    }

    #[test]
    fn get_session_mut() {
        let manager = SessionManager::new();

        // Create a session
        let session = manager.create_session("192.168.1.1:8080");
        let id = session.id;

        // Get a mutable reference and modify the session
        {
            let session_mut = manager.get_session(id);
            assert!(session_mut.is_some());

            let mut session_mut = session_mut.unwrap();
            // Store the initial last_activity
            let initial_time = session_mut.last_activity;

            // Modify the session
            session_mut.touch();

            // Verify the last_activity was updated
            assert!(session_mut.last_activity > initial_time);
        }

        // Verify getting a non-existent session returns None
        assert!(manager.get_session(12345).is_none());
    }

    #[test]
    fn session_new_default() {
        // Test the Session::new_default method
        let origin = "192.168.1.1:8080";
        let session = Session::new_default(origin);

        // Verify the origin was set correctly
        assert_eq!(session.origin, origin);

        // Create a second session with the same origin
        let session2 = Session::new_default(origin);

        // With new_default and same origin, they should have the same ID
        assert_eq!(session.id, session2.id);

        // Verify the timestamps were initialized
        assert!(session.start_time <= session.last_activity);
    }
}
