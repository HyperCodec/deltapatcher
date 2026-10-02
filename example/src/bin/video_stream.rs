use std::ops::RangeBounds;
use deltapatcher::{
    Differentiable,
    delta::SliceDelta,
    timeline::{Commit, StateCachedTimeline},
};

const FRAME_SIZE: usize = 10;
type Pixel = u8;
type Frame = [Pixel; FRAME_SIZE];
type RawVideo = Vec<Frame>;
type FrameDelta = SliceDelta<Pixel>;

fn epic_video() -> RawVideo {
    vec![
        [1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
        [2, 3, 4, 5, 6, 7, 8, 9, 10, 1],
        [3, 4, 5, 6, 7, 8, 9, 10, 1, 2],
        [4, 5, 6, 7, 8, 9, 10, 1, 2, 3],
        [8, 1, 5, 2, 3, 6, 7, 9, 10, 4],
        [10, 9, 8, 7, 6, 5, 4, 3, 2, 1],
        [8, 10, 4, 7, 6, 5, 2, 9, 3, 1],
    ]
}

struct VideoServer {
    timeline: StateCachedTimeline<Frame, FrameDelta>,
}

impl VideoServer {
    /// Fast range diffing in O(interval) time instead of O(N) delta aggregation.
    fn diff_across_many_fast(&self, range: impl RangeBounds<usize>) -> Option<FrameDelta> {
        self.timeline.get_aggregate_via_diff(range)
    }

    /// Returns a direct diff between any two frame indices (forward or backward).
    /// Used by the player to request lightweight delta payloads during seek operations.
    fn delta_between_frames(&self, from_frame: usize, to_frame: usize) -> Option<FrameDelta> {
        self.timeline.delta_between(from_frame, to_frame)
    }
}

impl From<RawVideo> for VideoServer {
    fn from(video: RawVideo) -> Self {
        let initial_frame = video[0];
        let commits = video
            .windows(2)
            .map(|w| Commit::new_with_default(w[1].differentiate(&w[0])));

        Self {
            timeline: StateCachedTimeline::from_commits(3, initial_frame, commits),
        }
    }
}

struct VideoPlayer<'a> {
    server: &'a VideoServer,
    current_frame: Frame,
    current_frame_index: usize,
}

impl VideoPlayer<'_> {
    /// Unified seek function for both forward and backward playback.
    ///
    /// Instead of sending an entire raw keyframe when scrubbing backwards,
    /// the server computes a lightweight inverted delta from `current_frame_index`
    /// to `target_frame`, keeping network payload minimal.
    fn skip_to_frame(&mut self, target_frame: usize) {
        if target_frame == self.current_frame_index {
            return;
        }

        // Fetch forward or backward delta from server
        let delta = self
            .server
            .delta_between_frames(self.current_frame_index, target_frame)
            .expect("invalid target frame index");

        // Patch player's current local state with the inverse/forward delta
        self.current_frame.patch(&delta);
        self.current_frame_index = target_frame;
    }

    fn display_current_frame(&self) {
        println!("Frame {:2}: {:?}", self.current_frame_index, self.current_frame);
    }
}

impl<'a> From<&'a VideoServer> for VideoPlayer<'a> {
    fn from(server: &'a VideoServer) -> Self {
        Self {
            server,
            current_frame: server.timeline.get_state_before(0).unwrap(),
            current_frame_index: 0,
        }
    }
}

fn main() {
    let raw = epic_video();
    let server = VideoServer::from(raw);
    let mut player = VideoPlayer::from(&server);

    player.display_current_frame();

    println!("\n=== Scrubbing Forward via Delta ===");
    player.skip_to_frame(5);
    player.display_current_frame();

    println!("\n=== Scrubbing Backward via Inverse Delta ===");
    // Player requests diff from Frame 5 -> Frame 2. Server calculates inverted delta.
    player.skip_to_frame(2);
    player.display_current_frame();

    println!("\n=== Fast Aggregation Comparison ===");
    let range_diff = server.diff_across_many_fast(1..=4).unwrap();
    println!("Direct O(interval) range diff (1..=4): {:?}", range_diff);
}