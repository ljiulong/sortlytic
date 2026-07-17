import { execFileSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { pathToFileURL } from 'node:url'

const nonAsciiPattern = /[^\x00-\x7F]/u

export function hasNonAscii(value) {
  return typeof value === 'string' && nonAsciiPattern.test(value)
}

export function validateReleaseTitles(titles, { subject = 'release metadata' } = {}) {
  const invalidTitles = titles.filter((title) => hasNonAscii(title))
  if (invalidTitles.length === 0) return

  const preview = invalidTitles
    .slice(0, 5)
    .map((title) => `- ${title}`)
    .join('\n')
  const remaining = invalidTitles.length > 5 ? `\n- ... and ${invalidTitles.length - 5} more` : ''
  throw new Error(
    `Release blocked: ${subject} must use ASCII characters.\n${preview}${remaining}`,
  )
}

function readGitHubEvent() {
  const eventPath = process.env.GITHUB_EVENT_PATH
  if (!eventPath) return {}

  try {
    return JSON.parse(readFileSync(eventPath, 'utf8'))
  } catch (error) {
    throw new Error(`Unable to read GitHub event payload: ${error instanceof Error ? error.message : String(error)}`)
  }
}

function readPushCommitTitles(event) {
  const before = event.before
  const after = event.after || process.env.GITHUB_SHA || 'HEAD'
  const cutover = execFileSync(
    'git',
    ['log', '--diff-filter=A', '--format=%H', '--reverse', '--', ':(top)scripts/validate-english-release.mjs'],
    { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] },
  ).trim().split(/\r?\n/u)[0]
  const isAncestor = (ancestor, descendant) => {
    try {
      execFileSync('git', ['merge-base', '--is-ancestor', ancestor, descendant], {
        stdio: ['ignore', 'ignore', 'ignore'],
      })
      return true
    } catch {
      return false
    }
  }
  const range = selectPushRange({ after, before, cutover, isAncestor })

  try {
    const output = execFileSync('git', ['log', '--format=%s', '--no-decorate', range], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
    })
    return output.split(/\r?\n/).map((title) => title.trim()).filter(Boolean)
  } catch (error) {
    throw new Error(
      `Unable to inspect pushed commits for range ${range}: ${error instanceof Error ? error.message : String(error)}`,
    )
  }
}

export function selectPushRange({ after, before, cutover, isAncestor }) {
  if (!cutover) return before && !/^0+$/u.test(before) ? `${before}..${after}` : after

  const cutoverReached = isAncestor(cutover, after)
  const remoteIncludesCutover = before
    && !/^0+$/u.test(before)
    && isAncestor(cutover, before)
  if (cutoverReached && !remoteIncludesCutover) return `${cutover}^..${after}`

  return before && !/^0+$/u.test(before) ? `${before}..${after}` : after
}

export function collectReleaseTitles({ eventName = process.env.GITHUB_EVENT_NAME, event = readGitHubEvent() } = {}) {
  if (eventName === 'pull_request') {
    const title = event.pull_request?.title ?? process.env.PR_TITLE
    if (!title) throw new Error('Release blocked: the pull request title is unavailable.')
    return { subject: 'pull request title', titles: [title] }
  }

  if (eventName === 'push') {
    return { subject: 'commit titles', titles: readPushCommitTitles(event) }
  }

  throw new Error(`Release metadata validation does not support event: ${eventName || 'unknown'}.`)
}

export function main() {
  const { subject, titles } = collectReleaseTitles()
  validateReleaseTitles(titles, { subject })
  console.log(`English release metadata validation passed for ${titles.length} ${subject}.`)
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    main()
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error))
    process.exitCode = 1
  }
}
