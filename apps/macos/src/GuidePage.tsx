import type { TFunction } from 'i18next'
import { useState } from 'react'
import {
  BadgeCheck,
  CheckCircle2,
  ExternalLink as ExternalLinkIcon,
  KeyRound,
  MonitorCheck,
  ShieldCheck,
} from 'lucide-react'
import { useTranslation } from 'react-i18next'
import ExternalLink, { type AllowedExternalUrl } from './ExternalLink'
import { i18n } from './i18n'
import './GuidePage.css'

type GuidePageProps = {
  onOpenSettings: () => void
}

type GuideChapter = {
  id: string
  title: string
  summary: string
  procedures: Array<{ title: string; detail: string }>
  facts?: Array<{ label: string; value: string }>
  action?: 'settings'
}

const chapterDefinitions = [
  {
    id: 'workspace',
    translationKey: 'workspace',
    procedures: ['environment', 'credentials', 'sample'],
    facts: ['platforms', 'localData', 'newTask', 'firstSample'],
  },
  {
    id: 'tikhub',
    translationKey: 'tikhub',
    procedures: ['register', 'token', 'domain', 'test'],
    facts: ['header', 'format', 'storage', 'pricing'],
    action: 'settings',
  },
  {
    id: 'model',
    translationKey: 'model',
    procedures: ['add', 'validate', 'switch', 'boundary'],
    facts: ['capabilities', 'validation', 'tasks', 'secrets'],
  },
  {
    id: 'create-task',
    translationKey: 'createTask',
    procedures: ['entry', 'target', 'region', 'filters', 'limits', 'review'],
    facts: ['generation', 'region', 'age', 'gender'],
  },
  {
    id: 'run-task',
    translationKey: 'runTask',
    procedures: ['edit', 'confirm', 'status', 'cancel', 'delete', 'failure'],
    facts: ['cancel', 'delete', 'partial', 'confirmation'],
  },
  {
    id: 'export',
    translationKey: 'export',
    procedures: ['format', 'generate', 'excel', 'pdf', 'backup'],
    facts: ['excel', 'pdf', 'evidence', 'boundary'],
  },
] as const

const safetyCheckKeys = ['secrets', 'filters', 'confirm', 'delete', 'backup'] as const
const troubleshootingKeys = ['tikhub', 'region', 'confirm', 'results', 'export'] as const
const officialResourceDefinitions = [
  { href: 'https://user.tikhub.io/register', translationKey: 'register' },
  { href: 'https://user.tikhub.io/login', translationKey: 'login' },
  { href: 'https://docs.tikhub.io/', translationKey: 'docs' },
  { href: 'https://tikhub.io/getting-started', translationKey: 'gettingStarted' },
  { href: 'https://tikhub.io/pricing', translationKey: 'pricing' },
] as const

function createGuideChapters(t: TFunction<'guide'>): GuideChapter[] {
  return chapterDefinitions.map((chapter) => {
    const prefix = `chapters.${chapter.translationKey}`

    return {
      id: chapter.id,
      title: t(`${prefix}.title`),
      summary: t(`${prefix}.summary`),
      procedures: chapter.procedures.map((procedure) => ({
        title: t(`${prefix}.procedures.${procedure}.title`),
        detail: t(`${prefix}.procedures.${procedure}.detail`),
      })),
      facts: chapter.facts.map((fact) => ({
        label: t(`${prefix}.facts.${fact}.label`),
        value: t(`${prefix}.facts.${fact}.value`),
      })),
      action: 'action' in chapter ? chapter.action : undefined,
    }
  })
}

function GuidePage({ onOpenSettings }: GuidePageProps) {
  const { t } = useTranslation('guide', { i18n })
  const { t: tCommon } = useTranslation('common', { i18n })
  const [externalLinkError, setExternalLinkError] = useState<string | null>(null)
  const guideChapters = createGuideChapters(t)
  const safetyChecks = safetyCheckKeys.map((key) => t(`safety.items.${key}`))
  const troubleshootingItems = troubleshootingKeys.map((key) => ({
    symptom: t(`troubleshooting.items.${key}.symptom`),
    action: t(`troubleshooting.items.${key}.action`),
  }))
  const officialResources = officialResourceDefinitions.map((resource) => ({
    href: resource.href,
    label: t(`resources.${resource.translationKey}`),
  }))

  return (
    <section className="guide-page" aria-label={t('aria.page')}>
      <header className="guide-intro">
        <div className="guide-intro__copy">
          <p className="eyebrow">{t('intro.eyebrow')}</p>
          <h2>{t('intro.title')}</h2>
          <p>{t('intro.description')}</p>
        </div>
        <span className="status-pill" data-tone="info">
          <MonitorCheck size={13} aria-hidden="true" />
          {t('intro.workspaceStatus')}
        </span>
        <dl className="guide-intro__facts">
          <div><dt>{t('intro.cost.label')}</dt><dd>{t('intro.cost.value')}</dd></div>
          <div><dt>{t('intro.region.label')}</dt><dd>{t('intro.region.value')}</dd></div>
          <div><dt>{t('intro.delivery.label')}</dt><dd>{t('intro.delivery.value')}</dd></div>
        </dl>
      </header>

      <nav className="guide-index" aria-label={t('aria.index')}>
        <div>
          <p className="eyebrow">{t('index.eyebrow')}</p>
          <strong>{t('index.order')}</strong>
        </div>
        <ol>
          {guideChapters.map((chapter, index) => (
            <li key={chapter.id}>
              <a href={`#guide-${chapter.id}`}>
                <span>{String(index + 1).padStart(2, '0')}</span>
                {chapter.title}
              </a>
            </li>
          ))}
        </ol>
      </nav>

      <main className="guide-handbook">
        <ol className="guide-chapters">
          {guideChapters.map((chapter, index) => (
            <li className="guide-chapter" id={`guide-${chapter.id}`} key={chapter.id}>
              <div className="guide-chapter__rail" aria-hidden="true">
                <span>{String(index + 1).padStart(2, '0')}</span>
              </div>
              <article className="guide-chapter__content">
                <header>
                  <p className="eyebrow">{t('stageLabel', { number: index + 1 })}</p>
                  <h2>{chapter.title}</h2>
                  <p>{chapter.summary}</p>
                </header>
                <ol className="guide-procedure">
                  {chapter.procedures.map((procedure, procedureIndex) => (
                    <li key={procedure.title}>
                      <span>{procedureIndex + 1}</span>
                      <strong>{procedure.title}</strong>
                      <p>{procedure.detail}</p>
                    </li>
                  ))}
                </ol>
                {chapter.facts ? (
                  <dl className="guide-facts">
                    {chapter.facts.map((fact) => (
                      <div key={fact.label}>
                        <dt>{fact.label}</dt>
                        <dd>{fact.value}</dd>
                      </div>
                    ))}
                  </dl>
                ) : null}
                {chapter.action === 'settings' ? (
                  <button className="ghost-button" type="button" onClick={onOpenSettings}>
                    <KeyRound size={16} aria-hidden="true" />
                    {t('actions.openSettings')}
                  </button>
                ) : null}
              </article>
            </li>
          ))}
        </ol>
      </main>

      <section className="guide-troubleshooting" aria-labelledby="guide-troubleshooting-heading">
        <header>
          <p className="eyebrow">{t('troubleshooting.eyebrow')}</p>
          <h2 id="guide-troubleshooting-heading">{t('troubleshooting.title')}</h2>
        </header>
        <dl>
          {troubleshootingItems.map((item) => (
            <div key={item.symptom}>
              <dt>{item.symptom}</dt>
              <dd>{item.action}</dd>
            </div>
          ))}
        </dl>
      </section>

      <div className="guide-reference-grid">
        <section className="guide-checklist" aria-labelledby="guide-checklist-heading">
          <header>
            <div>
              <p className="eyebrow">{t('safety.eyebrow')}</p>
              <h2 id="guide-checklist-heading">{t('safety.title')}</h2>
            </div>
            <ShieldCheck size={19} aria-hidden="true" />
          </header>
          <ul>
            {safetyChecks.map((item) => (
              <li key={item}>
                <CheckCircle2 size={16} aria-hidden="true" />
                <span>{item}</span>
              </li>
            ))}
          </ul>
        </section>

        <section className="guide-resources" aria-labelledby="guide-resources-heading">
          <header>
            <div>
              <p className="eyebrow">{t('resources.eyebrow')}</p>
              <h2 id="guide-resources-heading">{t('resources.title')}</h2>
            </div>
            <BadgeCheck size={18} aria-hidden="true" />
          </header>
          <nav aria-label={t('aria.resources')}>
            {officialResources.map((resource) => (
              <GuideLink
                {...resource}
                key={resource.href}
                onOpenError={() => setExternalLinkError(tCommon('unknownError'))}
                onOpenStart={() => setExternalLinkError(null)}
              />
            ))}
          </nav>
          {externalLinkError ? (
            <p className="guide-resources__error" role="status">
              {externalLinkError}
            </p>
          ) : null}
        </section>
      </div>
    </section>
  )
}

function GuideLink({
  href,
  label,
  onOpenError,
  onOpenStart,
}: {
  href: AllowedExternalUrl
  label: string
  onOpenError: () => void
  onOpenStart: () => void
}) {
  return (
    <ExternalLink
      className="guide-resource-link"
      href={href}
      onClick={onOpenStart}
      onOpenError={onOpenError}
    >
      <span>{label}</span>
      <ExternalLinkIcon size={15} aria-hidden="true" />
    </ExternalLink>
  )
}

export default GuidePage
