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

    sound_data: HashMap<Sound, Cursor<Vec<u8>>>,
}

impl Player {
    pub fn new() -> Result<Player> {
        let (_stream, stream_handle) = OutputStream::try_default()?;

        let put_token_white_bytes =
            Cursor::new(include_bytes!("../../../res/token_put_white.ogg").to_vec());
        let put_token_black_bytes =
            Cursor::new(include_bytes!("../../../res/token_put_black.ogg").to_vec());

        let p = Player {
            sound_data: HashMap::from([
                (Sound::PutToken(Side::White), put_token_white_bytes),
                (Sound::PutToken(Side::Black), put_token_black_bytes),
            ]),
            _stream,
            stream_handle,
        };

        Ok(p)
    }

    /// Plays the requested sound.
    pub fn play(&self, sound: Sound) -> Result<()> {
        // TODO: don't clone the whole vector.
        let source = Decoder::new(self.sound_data[&sound].clone())?;
        self.stream_handle.play_raw(source.convert_samples())?;

        Ok(())
    }
}
