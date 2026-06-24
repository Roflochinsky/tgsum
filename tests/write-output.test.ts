import { describe, it, expect, afterEach } from 'vitest'
import { mkdtempSync, rmSync, readdirSync, readFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { writeUnits } from '../src/write-output.js'
import type { ExtractedUnit } from '../src/types.js'

let dir = ''
afterEach(() => { if (dir) rmSync(dir, { recursive: true, force: true }) })

describe('writeUnits', () => {
  it('writes one file per single-part unit, parts suffix for split units', () => {
    dir = mkdtempSync(join(tmpdir(), 'tgsum-'))
    const units: ExtractedUnit[] = [
      { chatName: 'A', messages: [{ id: 1, type: 'message', date: '2026-06-18T09:00:00', from: 'X', from_id: 'u1', text: 'hi' }] },
      { chatName: 'B', messages: Array.from({ length: 30 }, (_, i) => ({ id: i, type: 'message' as const, date: '2026-06-18T09:00:00', from: 'X', from_id: 'u1', text: 'y'.repeat(40) })) },
    ]
    const written = writeUnits(units, dir, 60)
    const files = readdirSync(dir).sort()
    expect(files).toContain('A.md')
    expect(files.some(f => f === 'B.part-1.md')).toBe(true)
    expect(written.length).toBe(files.length)
    expect(readFileSync(join(dir, 'A.md'), 'utf8')).toContain('# Чат: A')
  })
})
