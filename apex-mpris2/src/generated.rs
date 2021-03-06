// This code was autogenerated with `dbus-codegen-rust -i org.mpris. -m None -c nonblock`, see https://github.com/diwic/dbus-rs
#![allow(
    unused_imports,
    clippy::needless_borrow,
    clippy::needless_borrowed_reference
)]
use dbus::{self, arg, nonblock};

pub trait MediaPlayer2Player {
    fn next(&self) -> nonblock::MethodReply<()>;
    fn previous(&self) -> nonblock::MethodReply<()>;
    fn pause(&self) -> nonblock::MethodReply<()>;
    fn play_pause(&self) -> nonblock::MethodReply<()>;
    fn stop(&self) -> nonblock::MethodReply<()>;
    fn play(&self) -> nonblock::MethodReply<()>;
    fn seek(&self, offset: i64) -> nonblock::MethodReply<()>;
    fn set_position(&self, track_id: dbus::Path, position: i64) -> nonblock::MethodReply<()>;
    fn open_uri(&self, uri: &str) -> nonblock::MethodReply<()>;
    fn playback_status(&self) -> nonblock::MethodReply<String>;
    fn loop_status(&self) -> nonblock::MethodReply<String>;
    fn set_loop_status(&self, value: String) -> nonblock::MethodReply<()>;
    fn rate(&self) -> nonblock::MethodReply<f64>;
    fn set_rate(&self, value: f64) -> nonblock::MethodReply<()>;
    fn shuffle(&self) -> nonblock::MethodReply<bool>;
    fn set_shuffle(&self, value: bool) -> nonblock::MethodReply<()>;
    fn metadata(&self) -> nonblock::MethodReply<arg::PropMap>;
    fn volume(&self) -> nonblock::MethodReply<f64>;
    fn set_volume(&self, value: f64) -> nonblock::MethodReply<()>;
    fn position(&self) -> nonblock::MethodReply<i64>;
    fn minimum_rate(&self) -> nonblock::MethodReply<f64>;
    fn maximum_rate(&self) -> nonblock::MethodReply<f64>;
    fn can_go_next(&self) -> nonblock::MethodReply<bool>;
    fn can_go_previous(&self) -> nonblock::MethodReply<bool>;
    fn can_play(&self) -> nonblock::MethodReply<bool>;
    fn can_pause(&self) -> nonblock::MethodReply<bool>;
    fn can_seek(&self) -> nonblock::MethodReply<bool>;
    fn can_control(&self) -> nonblock::MethodReply<bool>;
}

impl<'a, T: nonblock::NonblockReply, C: ::std::ops::Deref<Target = T>> MediaPlayer2Player
    for nonblock::Proxy<'a, C>
{
    fn next(&self) -> nonblock::MethodReply<()> {
        self.method_call("org.mpris.MediaPlayer2.Player", "Next", ())
    }

    fn previous(&self) -> nonblock::MethodReply<()> {
        self.method_call("org.mpris.MediaPlayer2.Player", "Previous", ())
    }

    fn pause(&self) -> nonblock::MethodReply<()> {
        self.method_call("org.mpris.MediaPlayer2.Player", "Pause", ())
    }

    fn play_pause(&self) -> nonblock::MethodReply<()> {
        self.method_call("org.mpris.MediaPlayer2.Player", "PlayPause", ())
    }

    fn stop(&self) -> nonblock::MethodReply<()> {
        self.method_call("org.mpris.MediaPlayer2.Player", "Stop", ())
    }

    fn play(&self) -> nonblock::MethodReply<()> {
        self.method_call("org.mpris.MediaPlayer2.Player", "Play", ())
    }

    fn seek(&self, offset: i64) -> nonblock::MethodReply<()> {
        self.method_call("org.mpris.MediaPlayer2.Player", "Seek", (offset,))
    }

    fn set_position(&self, track_id: dbus::Path, position: i64) -> nonblock::MethodReply<()> {
        self.method_call(
            "org.mpris.MediaPlayer2.Player",
            "SetPosition",
            (track_id, position),
        )
    }

    fn open_uri(&self, uri: &str) -> nonblock::MethodReply<()> {
        self.method_call("org.mpris.MediaPlayer2.Player", "OpenUri", (uri,))
    }

    fn playback_status(&self) -> nonblock::MethodReply<String> {
        <Self as nonblock::stdintf::org_freedesktop_dbus::Properties>::get(
            &self,
            "org.mpris.MediaPlayer2.Player",
            "PlaybackStatus",
        )
    }

    fn loop_status(&self) -> nonblock::MethodReply<String> {
        <Self as nonblock::stdintf::org_freedesktop_dbus::Properties>::get(
            &self,
            "org.mpris.MediaPlayer2.Player",
            "LoopStatus",
        )
    }

    fn set_loop_status(&self, value: String) -> nonblock::MethodReply<()> {
        <Self as nonblock::stdintf::org_freedesktop_dbus::Properties>::set(
            &self,
            "org.mpris.MediaPlayer2.Player",
            "LoopStatus",
            value,
        )
    }

    fn rate(&self) -> nonblock::MethodReply<f64> {
        <Self as nonblock::stdintf::org_freedesktop_dbus::Properties>::get(
            &self,
            "org.mpris.MediaPlayer2.Player",
            "Rate",
        )
    }

    fn set_rate(&self, value: f64) -> nonblock::MethodReply<()> {
        <Self as nonblock::stdintf::org_freedesktop_dbus::Properties>::set(
            &self,
            "org.mpris.MediaPlayer2.Player",
            "Rate",
            value,
        )
    }

    fn shuffle(&self) -> nonblock::MethodReply<bool> {
        <Self as nonblock::stdintf::org_freedesktop_dbus::Properties>::get(
            &self,
            "org.mpris.MediaPlayer2.Player",
            "Shuffle",
        )
    }

    fn set_shuffle(&self, value: bool) -> nonblock::MethodReply<()> {
        <Self as nonblock::stdintf::org_freedesktop_dbus::Properties>::set(
            &self,
            "org.mpris.MediaPlayer2.Player",
            "Shuffle",
            value,
        )
    }

    fn metadata(&self) -> nonblock::MethodReply<arg::PropMap> {
        <Self as nonblock::stdintf::org_freedesktop_dbus::Properties>::get(
            &self,
            "org.mpris.MediaPlayer2.Player",
            "Metadata",
        )
    }

    fn volume(&self) -> nonblock::MethodReply<f64> {
        <Self as nonblock::stdintf::org_freedesktop_dbus::Properties>::get(
            &self,
            "org.mpris.MediaPlayer2.Player",
            "Volume",
        )
    }

    fn set_volume(&self, value: f64) -> nonblock::MethodReply<()> {
        <Self as nonblock::stdintf::org_freedesktop_dbus::Properties>::set(
            &self,
            "org.mpris.MediaPlayer2.Player",
            "Volume",
            value,
        )
    }

    fn position(&self) -> nonblock::MethodReply<i64> {
        <Self as nonblock::stdintf::org_freedesktop_dbus::Properties>::get(
            &self,
            "org.mpris.MediaPlayer2.Player",
            "Position",
        )
    }

    fn minimum_rate(&self) -> nonblock::MethodReply<f64> {
        <Self as nonblock::stdintf::org_freedesktop_dbus::Properties>::get(
            &self,
            "org.mpris.MediaPlayer2.Player",
            "MinimumRate",
        )
    }

    fn maximum_rate(&self) -> nonblock::MethodReply<f64> {
        <Self as nonblock::stdintf::org_freedesktop_dbus::Properties>::get(
            &self,
            "org.mpris.MediaPlayer2.Player",
            "MaximumRate",
        )
    }

    fn can_go_next(&self) -> nonblock::MethodReply<bool> {
        <Self as nonblock::stdintf::org_freedesktop_dbus::Properties>::get(
            &self,
            "org.mpris.MediaPlayer2.Player",
            "CanGoNext",
        )
    }

    fn can_go_previous(&self) -> nonblock::MethodReply<bool> {
        <Self as nonblock::stdintf::org_freedesktop_dbus::Properties>::get(
            &self,
            "org.mpris.MediaPlayer2.Player",
            "CanGoPrevious",
        )
    }

    fn can_play(&self) -> nonblock::MethodReply<bool> {
        <Self as nonblock::stdintf::org_freedesktop_dbus::Properties>::get(
            &self,
            "org.mpris.MediaPlayer2.Player",
            "CanPlay",
        )
    }

    fn can_pause(&self) -> nonblock::MethodReply<bool> {
        <Self as nonblock::stdintf::org_freedesktop_dbus::Properties>::get(
            &self,
            "org.mpris.MediaPlayer2.Player",
            "CanPause",
        )
    }

    fn can_seek(&self) -> nonblock::MethodReply<bool> {
        <Self as nonblock::stdintf::org_freedesktop_dbus::Properties>::get(
            &self,
            "org.mpris.MediaPlayer2.Player",
            "CanSeek",
        )
    }

    fn can_control(&self) -> nonblock::MethodReply<bool> {
        <Self as nonblock::stdintf::org_freedesktop_dbus::Properties>::get(
            &self,
            "org.mpris.MediaPlayer2.Player",
            "CanControl",
        )
    }
}

#[derive(Debug)]
pub struct MediaPlayer2PlayerSeeked {
    pub position: i64,
}

impl arg::AppendAll for MediaPlayer2PlayerSeeked {
    fn append(&self, i: &mut arg::IterAppend) {
        arg::RefArg::append(&self.position, i);
    }
}

impl arg::ReadAll for MediaPlayer2PlayerSeeked {
    fn read(i: &mut arg::Iter) -> Result<Self, arg::TypeMismatchError> {
        Ok(MediaPlayer2PlayerSeeked {
            position: i.read()?,
        })
    }
}

impl dbus::message::SignalArgs for MediaPlayer2PlayerSeeked {
    const INTERFACE: &'static str = "org.mpris.MediaPlayer2.Player";
    const NAME: &'static str = "Seeked";
}
