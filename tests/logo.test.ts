import { describe, it, expect } from 'vitest'
import { renderLogo } from '../src/logo.js'

describe('renderLogo', () => {
  it('plain mode: 6 lines of block art, no ANSI escapes', () => {
    const out = renderLogo(false)
    expect(out.split('\n')).toHaveLength(6)
    expect(out).toContain('█')
    expect(out).not.toContain('\x1b[')
  })

  it('color mode: wraps each line in a 24-bit ANSI gradient escape and resets', () => {
    const out = renderLogo(true)
    expect(out).toContain('\x1b[1;38;2;0;255;255m') // top line = cyan
    expect(out).toContain('\x1b[1;38;2;80;120;255m') // bottom line = blue
    expect(out).toContain('\x1b[0m')
  })
})
