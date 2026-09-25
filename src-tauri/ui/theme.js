// Desktop theme support (Omarchy). A classic script in <head>, so the theme the
// app injects as `window.__TGSUM_THEME__` applies before the first paint;
// app.js calls `tgsumApplyTheme` again whenever the desktop theme changes.
;(() => {
  const VARS = [
    '--bg', '--surface', '--surface-2', '--surface-3', '--border', '--border-strong',
    '--text', '--muted', '--accent', '--accent-text', '--accent-soft', '--accent-ring',
    '--on-accent', '--grad-a', '--grad-b', '--logo-a', '--logo-b', '--glow-a', '--glow-b',
    '--success', '--danger', '--danger-soft',
  ]

  const mix = (a, b, pct) => `color-mix(in srgb, ${a} ${pct}%, ${b})`

  // WCAG relative luminance / contrast of #rrggbb colors.
  function luminance(hex) {
    const channel = (i) => {
      const c = parseInt(hex.slice(1 + 2 * i, 3 + 2 * i), 16) / 255
      return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4
    }
    return 0.2126 * channel(0) + 0.7152 * channel(1) + 0.0722 * channel(2)
  }
  function contrast(a, b) {
    const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x)
    return (hi + 0.05) / (lo + 0.05)
  }

  window.tgsumApplyTheme = (theme) => {
    const root = document.documentElement
    for (const name of VARS) root.style.removeProperty(name)
    root.style.colorScheme = theme ? theme.mode : ''
    if (!theme) {
      delete root.dataset.theme
      return
    }
    root.dataset.theme = theme.source

    const c = theme.colors
    const { accent, background: bg, foreground: fg } = c
    const light = theme.mode === 'light'
    const dark = c.darker_background || c.dark_background || (light ? fg : bg)
    const red = c.red || c.color1
    const vars = {
      '--bg': bg,
      '--surface': c.lighter_background || (light ? mix(bg, '#ffffff', 45) : mix(bg, fg, 94)),
      '--surface-2': mix(bg, fg, 93),
      '--surface-3': mix(bg, fg, 87),
      '--border': mix(bg, fg, 84),
      '--border-strong': mix(bg, fg, 72),
      '--text': fg,
      '--muted': mix(fg, bg, 62),
      '--accent': accent,
      '--accent-text': accent,
      '--accent-soft': mix(accent, 'transparent', 16),
      '--accent-ring': mix(accent, 'transparent', 45),
      '--on-accent': contrast(accent, '#ffffff') >= contrast(accent, dark) ? '#ffffff' : dark,
      '--grad-a': mix(accent, fg, 72),
      '--grad-b': accent,
      '--logo-a': mix(accent, fg, 60),
      '--logo-b': accent,
      '--glow-a': mix(accent, 'transparent', 9),
      '--glow-b': mix(accent, 'transparent', 7),
      '--success': c.green || c.color2,
      '--danger': red,
      '--danger-soft': red && mix(red, 'transparent', 12),
    }
    for (const [name, value] of Object.entries(vars)) if (value) root.style.setProperty(name, value)
  }

  window.tgsumApplyTheme(window.__TGSUM_THEME__ || null)
})()
