import { describe, it, expect } from 'vitest'
import { styleItem } from '../src/select-prompt.js'
import { S_CHECKBOX_SELECTED, S_CHECKBOX_INACTIVE } from '@clack/prompts'

// styleText colors are TTY-gated (no escape codes in non-TTY test env), so we
// assert the TTY-independent structure: the active row carries a `›` pointer and
// rows use the selected/inactive checkbox glyphs. Green is verified visually.
const opt = { value: 'a', label: 'Чат А' }

describe('styleItem', () => {
  it('active (focused) row is marked with a pointer', () => {
    expect(styleItem(opt, true, [])).toContain('›')
  })
  it('selected row uses the selected checkbox and no pointer', () => {
    const s = styleItem(opt, false, ['a'])
    expect(s).toContain(S_CHECKBOX_SELECTED)
    expect(s).not.toContain('›')
  })
  it('inactive unselected row uses the empty checkbox and no pointer', () => {
    const s = styleItem(opt, false, [])
    expect(s).toContain(S_CHECKBOX_INACTIVE)
    expect(s).not.toContain('›')
  })
})
