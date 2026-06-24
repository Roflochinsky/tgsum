import { mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import type { ExtractedUnit } from './types.js'
import { formatUnit } from './format.js'

export function writeUnits(units: ExtractedUnit[], outDir: string, maxTokens = 90_000): string[] {
  mkdirSync(outDir, { recursive: true })
  const written: string[] = []
  // Track how many times each stem has been emitted so colliding filenames get
  // " (2)", " (3)", ... appended before the .part-N/.md suffix (no silent overwrite).
  const seen = new Map<string, number>()
  for (const unit of units) {
    const { filename, parts } = formatUnit(unit, maxTokens)
    const baseStem = filename.replace(/\.md$/, '')
    const n = (seen.get(baseStem) ?? 0) + 1
    seen.set(baseStem, n)
    const stem = n === 1 ? baseStem : `${baseStem} (${n})`
    if (parts.length === 1) {
      const p = join(outDir, `${stem}.md`)
      writeFileSync(p, parts[0], 'utf8')
      written.push(p)
    } else {
      parts.forEach((content, i) => {
        const p = join(outDir, `${stem}.part-${i + 1}.md`)
        writeFileSync(p, content, 'utf8')
        written.push(p)
      })
    }
  }
  return written
}
