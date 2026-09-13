hl.monitor({ output = "", mode = "1280x720@60", position = "auto", scale = 1 })
hl.config({
  misc = { disable_hyprland_logo = true, disable_splash_rendering = true },
  input = { kb_layout = "us" },
})
hl.bind("mouse:272", hl.dsp.event("opendesk-left-release"), {
  release = true,
  non_consuming = true,
  allow_input_capture = true,
  dont_inhibit = true,
})
hl.bind("CTRL + ALT + ESCAPE", hl.dsp.event("opendesk-emergency-release"), {
  non_consuming = true,
  dont_inhibit = true,
  allow_input_capture = true,
})
