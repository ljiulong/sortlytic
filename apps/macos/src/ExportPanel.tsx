import {
  FileSpreadsheet,
  FileText,
  Network,
  ShieldCheck,
  type LucideIcon,
} from 'lucide-react'
import { useTranslation } from 'react-i18next'

type ExportPanelProps = {
  isBusy: boolean
  latestExports: Array<{ export_type: string; file_path?: string | null; status: string }>
  onExport: () => Promise<unknown>
}

function ExportPanel({ isBusy, latestExports, onExport }: ExportPanelProps) {
  const { t } = useTranslation('tasks')
  const xlsxExport = latestExports.find((item) => item.export_type === 'xlsx')
  const pdfExport = latestExports.find((item) => item.export_type === 'pdf')
  const hasPassedExportGate = xlsxExport?.status === 'success' && pdfExport?.status === 'success'
  const statusLabel = hasPassedExportGate ? t('exportPanel.passed') : t('exportPanel.pending')

  return (
    <section className="glass-panel compact-panel" aria-labelledby="export-panel-heading">
      <div className="section-heading">
        <div>
          <p className="eyebrow">{t('exportPanel.eyebrow')}</p>
          <h2 id="export-panel-heading">{t('exportPanel.title')}</h2>
        </div>
        <span className="status-pill" data-tone={hasPassedExportGate ? 'success' : 'info'}>
          {statusLabel}
        </span>
      </div>
      <div className="export-grid">
        <ExportItem
          icon={FileSpreadsheet}
          label={t('export.formats.xlsx')}
          meta={xlsxExport?.file_path ?? t('exportPanel.waiting')}
          tone={xlsxExport?.status === 'success' ? 'success' : 'info'}
        />
        <ExportItem
          icon={FileText}
          label={t('export.formats.pdf')}
          meta={pdfExport?.file_path ?? t('exportPanel.waiting')}
          tone={pdfExport?.status === 'success' ? 'success' : 'warning'}
        />
        <ExportItem
          icon={Network}
          label={t('exportPanel.webhookLabel')}
          meta={t('exportPanel.webhookMeta')}
          tone="info"
        />
      </div>
      <button
        className="primary-button wide-button"
        disabled={isBusy}
        aria-label={t('exportPanel.runCheck')}
        type="button"
        onClick={() => {
          void onExport()
        }}
      >
        <ShieldCheck size={16} aria-hidden="true" />
        {t('exportPanel.runCheck')}
      </button>
    </section>
  )
}

function ExportItem({
  icon: Icon,
  label,
  meta,
  tone,
}: {
  icon: LucideIcon
  label: string
  meta: string
  tone: string
}) {
  return (
    <article className="export-item">
      <div className="connection-icon" data-tone={tone}>
        <Icon size={17} aria-hidden="true" />
      </div>
      <div>
        <strong>{label}</strong>
        <span>{meta}</span>
      </div>
    </article>
  )
}

export default ExportPanel
