Vendored from crates.io `winit` 0.30.13.

The only local change is removing the private macOS
`_CGSSetWindowBackgroundBlurRadius` import so Mac App Store review
(Guideline 2.5.1) can accept the binary. `Window::set_blur` is a no-op
on macOS in this tree.
