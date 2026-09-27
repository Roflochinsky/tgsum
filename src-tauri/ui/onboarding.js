// Only UI preferences and recent Project IDs are persisted here. No auth paths,
// credentials, source paths or content. Storage failure never blocks local work.
let preferences = {}
try { preferences = JSON.parse(localStorage.getItem('tgsum.ui.v1') || '{}') || {} } catch { /* defaults */ }
if (typeof preferences !== 'object' || Array.isArray(preferences)) preferences = {}
const save = () => { try { localStorage.setItem('tgsum.ui.v1', JSON.stringify(preferences)) } catch { /* session only */ } }
export const preferredAgent = () => ['codex', 'claude'].includes(preferences.agent) ? preferences.agent : 'export'
export const recentProjects = () => Array.isArray(preferences.recent) ? preferences.recent.filter((id) => typeof id === 'string').slice(0, 20) : []
export function rememberProject(id) {
  preferences.recent = [id, ...recentProjects().filter((v) => v !== id)].slice(0, 20)
  save()
}

export function mountOnboarding({ invoke, busy }) {
  const $ = (s) => document.querySelector(s)
  const dialog = $('#onboarding')
  let step = 0
  let epoch = 0
  function render() {
    for (const panel of dialog.querySelectorAll('[data-onboarding]')) panel.hidden = Number(panel.dataset.onboarding) !== step
    $('#onboarding-progress').textContent = `Знакомство с TGSUM · ${step + 1} из 3`
    $('#onboarding-back').disabled = step === 0
    $('#onboarding-next').textContent = step === 2 ? 'Готово · к проектам' : 'Далее'
  }
  async function open() {
    if (busy() || dialog.open) return
    step = 0
    render()
    dialog.showModal()
    const request = ++epoch
    const select = $('#onboarding-agent')
    select.replaceChildren(new Option('Export only · сохранить локально', 'export'))
    $('#onboarding-agents').textContent = 'Проверяем доступные программы…'
    try {
      const catalog = await invoke('analysis_catalog')
      if (request !== epoch || !dialog.open) return
      $('#onboarding-agents').replaceChildren(...catalog.agents.map((agent) => {
        select.add(new Option(agent.title, agent.id))
        const item = document.createElement('li')
        item.textContent = `${agent.title}: ${agent.executables.length ? 'найден путь к программе' : 'путь нужно выбрать вручную'}. ${agent.available ? 'Совместимость ещё не проверена.' : 'Запуск в этой среде недоступен.'}`
        return item
      }))
      select.value = preferredAgent()
    } catch {
      $('#onboarding-agents').textContent = 'Не удалось проверить программы. Сохранение контекста доступно; агент можно выбрать позже.'
    }
  }
  $('#onboarding-back').addEventListener('click', () => { step = Math.max(0, step - 1); render() })
  $('#onboarding-next').addEventListener('click', () => {
    if (step < 2) { step++; render(); return }
    preferences.agent = $('#onboarding-agent').value
    preferences.seen = true
    save()
    dialog.close()
  })
  dialog.addEventListener('close', () => { epoch++ })
  $('#btn-help').addEventListener('click', open)
  return { firstRun: () => { if (!preferences.seen) return open() } }
}
