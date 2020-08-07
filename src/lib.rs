use webutil::channel::{channel, Sender, oneshot, Oneshot};

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use libtetris::*;

#[cfg(feature = "wee_alloc")]
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[wasm_bindgen]
pub struct CCInterface {
    send: Sender<(InterfaceCommand, Oneshot<Option<Option<(cold_clear::Move, cold_clear::Info)>>>)>
}

#[derive(Debug)]
struct ArgumentError<T>(T);

enum InterfaceCommand {
    Reset {
        field: [[bool; 10]; 40],
        b2b: bool,
        combo: u32
    },
    NewPiece(Piece),
    NextMove(u32),
    ForceAnalysisLine(Vec<FallingPiece>)
}

enum WorkerState {
    Initializing(Board, u32),
    Ready(cold_clear::Interface)
}

fn to_js_error<E: std::fmt::Debug>(error: E) -> JsValue {
    let js_error = js_sys::Error::new(&format!("{:?}", error));
    js_error.set_name(std::any::type_name::<E>());
    js_error.dyn_into().unwrap()
}

#[wasm_bindgen]
impl CCInterface {
    /// Launches a bot worker with the specified starting board and options.
    pub fn launch(options: JsValue, evaluator: JsValue) -> Result<CCInterface, JsValue> {
        #[cfg(feature = "console_error_panic_hook")]
        console_error_panic_hook::set_once();
        let options: cold_clear::Options = options
            .into_serde()
            .map_err(to_js_error)?;
        let evaluator: cold_clear::evaluation::Standard = evaluator
            .into_serde()
            .map_err(to_js_error)?;
        let mut interface_args = Some((options, evaluator));
        let (send, recv) = channel::<(_, Oneshot<_>)>();
        wasm_bindgen_futures::spawn_local(async move {
            let mut state = WorkerState::Initializing(Board::new(), if options.use_hold { 3 } else { 2 });
            while let Some((command, send)) = recv.recv().await {
                send.resolve(match &mut state {
                    WorkerState::Initializing(board, pieces_left) => {
                        if let InterfaceCommand::NewPiece(piece) = command {
                            board.add_next_piece(piece);
                            *pieces_left -= 1;
                            if *pieces_left == 0 {
                                let (options, evaluator) = interface_args.take().unwrap();
                                let interface = cold_clear::Interface::launch(
                                    board.clone(),
                                    options,
                                    evaluator
                                ).await;
                                state = WorkerState::Ready(interface);
                            }
                        }
                        None
                    }
                    WorkerState::Ready(interface) => {
                        match command {
                            InterfaceCommand::Reset { field, b2b, combo } => {
                                interface.reset(field, b2b, combo);
                                None
                            }
                            InterfaceCommand::NewPiece(piece) => {
                                interface.add_next_piece(piece);
                                None
                            }
                            InterfaceCommand::NextMove(incoming) => {
                                interface.request_next_move(incoming);
                                Some(interface.next_move().await)
                            }
                            InterfaceCommand::ForceAnalysisLine(line) => {
                                interface.force_analysis_line(line);
                                None
                            }
                        }
                    }
                }).unwrap();
            }
        });
        Ok(Self { send })
    }
    
    /// Request the bot to provide a move as soon as possible.
    /// 
    /// In most cases, "as soon as possible" is a very short amount of time, and is only longer if
    /// the provided lower limit on thinking has not been reached yet or if the bot cannot provide
    /// a move yet, usually because it lacks information on the next pieces.
    /// 
    /// For example, in a game with zero piece previews and hold enabled, the bot will never be able
    /// to provide the first move because it cannot know what piece it will be placing if it chooses
    /// to hold. Another example: in a game with zero piece previews and hold disabled, the bot
    /// will only be able to provide a move after the current piece spawns and you provide the piece
    /// information to the bot using `add_next_piece`.
    /// 
    /// It is recommended that you call this function the frame before the piece spawns so that the
    /// bot has time to finish its current thinking cycle and supply the move.
    /// 
    /// Once a move is chosen, the bot will update its internal state to the result of the piece
    /// being placed correctly and the returned promise will resolve with the move. If the promise
    /// returns `null`, the bot has died.
    pub fn next_move(&self, incoming: u32) -> js_sys::Promise {
        self.command(InterfaceCommand::NextMove(incoming))
    }
    
    /// Adds a new piece to the end of the queue.
    /// 
    /// If speculation is enabled, the piece *must* be in the bag. For example, if in the current
    /// bag you've provided the sequence IJOZT, then the next time you call this function you can
    /// only provide either an L or an S piece.
    pub fn add_next_piece(&mut self, piece: JsValue) -> Result<js_sys::Promise, JsValue> {
        let piece = piece
            .into_serde()
            .map_err(to_js_error)?;
        Ok(self.command(InterfaceCommand::NewPiece(piece)))
    }

    /// Resets the playfield, back-to-back status, and combo count.
    /// 
    /// This should only be used when garbage is received or when your client could not place the
    /// piece in the correct position for some reason (e.g. 15 move rule), since this forces the
    /// bot to throw away previous computations.
    /// 
    /// Note: combo is not the same as the displayed combo in guideline games. Here, it is the
    /// number of consecutive line clears achieved. So, generally speaking, if "x Combo" appears
    /// on the screen, you need to use x+1 here.
    ///
    /// `field` is an array of 40 rows, which are arrays of 10 bools. The first element is the
    /// first row.
    pub fn reset(&self, field: JsValue, b2b_active: bool, combo: u32) -> Result<js_sys::Promise, JsValue> {
        let src_field: Vec<[bool; 10]> = field
            .into_serde()
            .map_err(to_js_error)?;
        let mut field = [[false; 10]; 40];
        if src_field.len() != field.len() {
            let message = format!("`field` must be 40 rows (got {})", src_field.len());
            Err(to_js_error(ArgumentError(message)))
        } else {
            for (src, dest) in src_field.into_iter().zip(field.iter_mut()) {
                *dest = src;
            }
            Ok(self.command(InterfaceCommand::Reset { field, b2b: b2b_active, combo }))
        }
    }

    /// Specifies a line that Cold Clear should analyze before making any moves.
    pub fn force_analysis_line(&self, path: JsValue) -> Result<js_sys::Promise, JsValue> {
        let path = path
            .into_serde()
            .map_err(to_js_error)?;
        Ok(self.command(InterfaceCommand::ForceAnalysisLine(path)))
    }

    fn command(&self, command: InterfaceCommand) -> js_sys::Promise {
        let (send, recv) = oneshot();
        // `Oneshot<T>` doesn't implement `Debug`, so in the meantime the error is discarded first.
        self.send.send((command, send))
            .map_err(|_| ())
            .unwrap();
        wasm_bindgen_futures::future_to_promise(async move {
            Ok(JsValue::from_serde(&recv.await).unwrap())
        })
    }
}

#[wasm_bindgen]
struct CCOptions;

#[wasm_bindgen]
impl CCOptions {
    pub fn default() -> JsValue {
        JsValue::from_serde(&cold_clear::Options::default()).unwrap()
    }
}

#[wasm_bindgen]
struct CCEvaluator;

#[wasm_bindgen]
impl CCEvaluator {
    pub fn default() -> JsValue {
        JsValue::from_serde(&cold_clear::evaluation::Standard::default()).unwrap()
    }
    pub fn fast_config() -> JsValue {
        JsValue::from_serde(&cold_clear::evaluation::Standard::fast_config()).unwrap()
    }
}
