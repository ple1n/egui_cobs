use tokio::sync::mpsc;

pub type MPSCRecv<S> = BufRecv<mpsc::Receiver<S>, S>;

pub struct BufRecv<IN, S: Merge> {
    recv: IN,
    state: S,
}

pub trait Merge: Sized {
    type Delta: Sized;
    fn merge(&mut self, incoming: Self::Delta);
}

impl<S: Merge> BufRecv<mpsc::Receiver<S::Delta>, S> {
    pub fn buf_recv(&mut self) -> &mut S {
        let t = self.recv.try_recv();
        match t {
            Result::Ok(t) => {
                self.state.merge(t);
            }
            _ => (),
        }
        &mut self.state
    }
    pub fn from_in(ix: mpsc::Receiver<S::Delta>) -> Self
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
pub struct EguiSender<T: Merge> {
    cc: egui::Context,
    sx: mpsc::Sender<T::Delta>,
}

impl<T: Merge> EguiSender<T> {
    pub async fn send(&self, t: T::Delta) -> Result<(), mpsc::error::SendError<T::Delta>> {
        self.sx.send(t).await?;
        self.cc.request_repaint();
        Ok(())
    }
}

pub fn mpsc_chan<D: Sized, T: Merge<Delta = D> + Default>(
    ctx: egui::Context,
    n: usize,
) -> (EguiSender<T>, BufRecv<mpsc::Receiver<D>, T>) {
    let (sx, rx) = mpsc::channel(n);
    let k: BufRecv<mpsc::Receiver<D>, T> = BufRecv::from_in(rx);
    (EguiSender { cc: ctx, sx }, k)
}
