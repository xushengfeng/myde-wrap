mod compositor;
mod xdg_shell;

use crate::wayland::App;

//
// Wl Seat
//

use smithay::input::{Seat, SeatHandler, SeatState};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::Resource;
use smithay::wayland::output::OutputHandler;
use smithay::wayland::selection::SelectionHandler;
use smithay::wayland::selection::data_device::{
    DataDeviceHandler, DataDeviceState, ClientDndGrabHandler, ServerDndGrabHandler, set_data_device_focus,
};

impl SeatHandler for App {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<App> {
        &mut self.seat_state
    }

    fn cursor_image(&mut self, _seat: &Seat<Self>, _image: smithay::input::pointer::CursorImageStatus) {}

    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&WlSurface>) {
        let dh = &self.display_handle;
        let client = focused.and_then(|s| dh.get_client(s.id()).ok());
        set_data_device_focus(dh, seat, client);
    }
}

//
// Wl Data Device
//

impl SelectionHandler for App {
    type SelectionUserData = ();
}

impl ClientDndGrabHandler for App {}
impl ServerDndGrabHandler for App {}

impl DataDeviceHandler for App {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}

//
// Wl Output & Xdg Output
//

impl OutputHandler for App {}

// Delegate macros
smithay::delegate_compositor!(App);
smithay::delegate_shm!(App);
smithay::delegate_seat!(App);
smithay::delegate_xdg_shell!(App);
smithay::delegate_output!(App);
smithay::delegate_data_device!(App);
