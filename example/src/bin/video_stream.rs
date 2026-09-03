use deltapatcher::{Differentiable, delta::SliceDelta, timeline::Timeline};

const FRAME_SIZE: usize = 10;

type Pixel = u8;
type Frame = [Pixel; FRAME_SIZE];

type RawVideo = Vec<Frame>;

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

    // we could also add things like timestamp to commit metadata
    timeline: Timeline<SliceDelta<Pixel>>,
    // TODO data structure that caches states at certain intervals for faster backwards traversal.
}

impl VideoServer {
    fn request_next_frame(&self, current: usize) -> &SliceDelta<Pixel> {
        &self.timeline[current]
    }
}

impl From<RawVideo> for VideoServer {
    fn from(video: RawVideo) -> Self {
        Self {
            initial_frame: video[0],

            // this is an embarassingly parallel problem:
            // we can get good parallel efficiency by simply
            // changing windows(2) to rayon's par_windows(2).
            timeline: video
                .windows(2) // windows(2) groups them into twos, i.e. [0, 1], [1, 2], [2, 3].
                .map(|w| w[1].differentiate(&w[0]))
                .collect(),
        }
    }
}

struct VideoPlayer<'a> {
    server: &'a VideoServer,
    current_frame: Frame,
    current_frame_index: usize,
}

impl VideoPlayer<'_> {
    fn next_frame(&mut self) {
        let delta = self.server.request_next_frame(self.current_frame_index);
        self.current_frame.patch(delta);
        self.current_frame_index += 1;
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
}