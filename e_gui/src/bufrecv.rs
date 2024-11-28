use tokio::sync::mpsc;

pub type MPSCRecv<S> = BufRecv<mpsc::Receiver<S>, S>;

pub struct BufRecv<IN, S> {
    recv: IN,
    state: S,
}

impl<S> BufRecv<mpsc::Receiver<S>, S> {
    pub fn buf_recv(&mut self) -> &mut S {
        let t = self.recv.try_recv();
        match t {
            Result::Ok(t) => {
                self.state = t;
            }
            _ => (),
        }
        &mut self.state
    }
    pub fn from_in(ix: mpsc::Receiver<S>) -> Self
    where
        S: Default,
    {
        Self {
            recv: ix,
            state: S::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct EguiSender<T> {
    cc: egui::Context,
    sx: mpsc::Sender<T>,
}

impl<T> EguiSender<T> {
    pub async fn send(&self, t: T) -> Result<(), mpsc::error::SendError<T>> {
        self.sx.send(t).await?;
        self.cc.request_repaint();
        Ok(())
    }
}

pub fn mpsc_chan<T: Default>(
    ctx: egui::Context,
    n: usize,
) -> (EguiSender<T>, BufRecv<mpsc::Receiver<T>, T>) {
    let (sx, rx) = mpsc::channel(n);
    (EguiSender { cc: ctx, sx }, BufRecv::from_in(rx))
}
