use std::ops::RangeBounds;

use deltapatcher::{Differentiable, delta::SliceDelta, timeline::{Commit, StateCachedTimeline}};

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
    initial_frame: Frame,

    // a timeline is a data structure representing a sequence
    // of "commits" which are comprised of a delta and some optional arbitrary metadata.
    // a state-cached timeline stores the actual state at certain intervals,
    // making retrieval of arbitrary states a constant-time operation.
    timeline: StateCachedTimeline<Frame, FrameDelta>,
    // TODO data structure that caches states at certain intervals for faster backwards traversal.
}

impl VideoServer {
    /// Gets the delta pointing to the next frame
    /// after the one at the index specified by current.
    fn next_frame(&self, current: usize) -> &FrameDelta {
        &self.timeline[current]
    }

    /// Returning a delta over the range of frames.
    /// Returns `None` if any part of the range is out of bounds.
    fn merge_frames(&self, range: impl RangeBounds<usize>) -> Option<FrameDelta> {
        self.timeline.get_aggregate(range)
    }

    /// get the exact frame at a certain index in O(1).
    /// returns `None` if the index is out of bounds.
    fn get_frame(&self, i: usize) -> Option<Frame> {
        self.timeline.get_state_after(i)
    }
}

impl From<RawVideo> for VideoServer {
    fn from(video: RawVideo) -> Self {
        // this is an embarassingly parallel problem:
        // we can get good parallel efficiency by simply
        // changing windows(2) to rayon's par_windows(2) and
        // collecting to a Vec before passing it to `from_commits_with_default`.
        let commits = video
            .windows(2) // windows(2) groups them into twos, i.e. [0, 1], [1, 2], [2, 3].
            .map(|w| Commit::new_with_default(w[1].differentiate(&w[0])));
        
        Self {
            initial_frame: video[0],
            timeline: StateCachedTimeline::from_commits_with_default(3, commits),
        }
    }
}

struct VideoPlayer<'a> {
    // pretend that this is a request client
    // for some real server. calling its methods
    // would induce latency and overhead depending primarily
    // on the amount of data being sent/received.
    // thus, it is important to minimize how much data we transfer.
    server: &'a VideoServer,
    current_frame: Frame,
    current_frame_index: usize,
}

impl VideoPlayer<'_> {
    fn next_frame(&mut self) {
        let delta = self.server.next_frame(self.current_frame_index);
        self.current_frame.patch(delta);
        self.current_frame_index += 1;
    }

    fn skip_to_frame(&mut self, frame: usize) {
        if frame > self.current_frame_index {
            let delta = self.server.merge_frames(self.current_frame_index+1..=frame)
                .expect("invalid next frame index");
            self.current_frame.patch(&delta);
            self.current_frame_index = frame;
        } else if frame < self.current_frame_index {
            // it's a lot harder to generate a backwards delta
            // since data is usually destroyed.
            self.current_frame = self.server.get_frame(frame).expect("invalid frame index");
            self.current_frame_index = frame;
        }
    }

    fn display_current_frame(&self) {
        println!("{:?}", self.current_frame);
    }
}

impl<'a> From<&'a VideoServer> for VideoPlayer<'a> {
    fn from(server: &'a VideoServer) -> Self {
        Self {
            server,
            current_frame: server.initial_frame,
            current_frame_index: 0,
        }
    }
}

fn main() {
    let raw = epic_video();
    let len = raw.len();

    let server = VideoServer::from(raw);
    let mut player = VideoPlayer::from(&server);

    player.display_current_frame();
    for _ in 1..len {
        player.next_frame();
        player.display_current_frame();
    }

    dbg!(&server.timeline);
}