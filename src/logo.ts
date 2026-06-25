// TGSUM banner (figlet "ANSI Shadow"), painted with a cyan→blue per-line
// gradient using raw 24-bit ANSI escapes — no color dependency.
const BANNER = [
  '████████╗ ██████╗ ███████╗██╗   ██╗███╗   ███╗',
  '╚══██╔══╝██╔════╝ ██╔════╝██║   ██║████╗ ████║',
  '   ██║   ██║  ███╗███████╗██║   ██║██╔████╔██║',
  '   ██║   ██║   ██║╚════██║██║   ██║██║╚██╔╝██║',
  '   ██║   ╚██████╔╝███████║╚██████╔╝██║ ╚═╝ ██║',
  '   ╚═╝    ╚═════╝ ╚══════╝ ╚═════╝ ╚═╝     ╚═╝',
]

const lerp = (a: number, b: number, t: number) => Math.round(a + (b - a) * t)

// Cyan (0,255,255) at the top → blue (80,120,255) at the bottom.
export function renderLogo(color = true): string {
  if (!color) return BANNER.join('\n')
  const [tr, tg, tb] = [0, 255, 255]
  const [br, bg, bb] = [80, 120, 255]
  return BANNER.map((line, i) => {
    const t = BANNER.length > 1 ? i / (BANNER.length - 1) : 0
    const r = lerp(tr, br, t), g = lerp(tg, bg, t), b = lerp(tb, bb, t)
    return `\x1b[1;38;2;${r};${g};${b}m${line}\x1b[0m`
  }).join('\n')
}

export function printLogo(): void {
  // ponytail: only colorize on a real TTY; honor NO_COLOR.
  const color = Boolean(process.stdout.isTTY) && !process.env.NO_COLOR
  process.stdout.write('\n' + renderLogo(color) + '\n\n')
}
