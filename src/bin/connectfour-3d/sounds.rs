use std::collections::HashMap;
use std::io::Cursor;

use anyhow::Result;
use rodio::{source::Source, Decoder, OutputStream, OutputStreamHandle};

use connectfour::game::Side;

/// Describes which sound effect to play.
#[derive(Eq, PartialEq, Hash)]
pub enum Sound {
    /// Played when someone puts a new token on the board.
    PutToken(Side),
}

/// Sound effects player. It embeds all the sound data in memory.
pub struct Player {
    /// Stream and its handle created by Rodio's try_default. NOTE: even though
    /// we don't use stream directly, we still have to retain it in this struct,
    /// otherwise it will be dropped after new() returns (that's obvious), and
    /// then play_raw will error out with NoDevice (not so obvious).
    _stream: OutputStream,
    stream_handle: OutputStreamHandle,

    sound_data: HashMap<Sound, &'static [u8]>,
}

impl Player {
    pub fn new() -> Result<Player> {
        let (_stream, stream_handle) = OutputStream::try_default()?;

        let p = Player {
            sound_data: HashMap::from([
                (
                    Sound::PutToken(Side::White),
                    include_bytes!("../../../res/token_put_white.ogg").as_slice(),
                ),
                (
                    Sound::PutToken(Side::Black),
                    include_bytes!("../../../res/token_put_black.ogg").as_slice(),
                ),
            ]),
            _stream,
            stream_handle,
        };

        Ok(p)
    }

    /// Plays the requested sound.
    pub fn play(&self, sound: Sound) -> Result<()> {
        let source = Decoder::new(Cursor::new(self.sound_data[&sound]))?;
        self.stream_handle.play_raw(source.convert_samples())?;

        Ok(())
    }
}
