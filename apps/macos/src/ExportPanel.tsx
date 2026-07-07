import {
  FileSpreadsheet,
  FileText,
  Network,
  ShieldCheck,
  type LucideIcon,
} from 'lucide-react'

type ExportPanelProps = {
  isBusy: boolean
  latestExports: Array<{ export_type: string; file_path?: string | null; status: string }>
  onExport: () => Promise<unknown>
}

function ExportPanel({ isBusy, latestExports, onExport }: ExportPanelProps) {
  const xlsxExport = latestExports.find((item) => item.export_type === 'xlsx')
  const pdfExport = latestExports.find((item) => item.export_type === 'pdf')
  const hasPassedExportGate = xlsxExport?.status === 'success' && pdfExport?.status === 'success'
  const statusLabel = hasPassedExportGate ? '已通过' : '待检查'

  return (
    <section className="glass-panel compact-panel">
      <div className="section-heading">
        <div>
          <p className="eyebrow">导出中心</p>
          <h2>Excel 与 PDF 门禁</h2>
        </div>
        <span className="status-pill" data-tone={hasPassedExportGate ? 'success' : 'info'}>
          {statusLabel}
        </span>
      </div>
      <div className="export-grid">
        <ExportItem
          icon={FileSpreadsheet}
          label="Excel 工作簿"
          meta={xlsxExport?.file_path ?? '等待生成'}
          tone={xlsxExport?.status === 'success' ? 'success' : 'info'}
        />
        <ExportItem
          icon={FileText}
          label="PDF 报告"
          meta={pdfExport?.file_path ?? '等待生成'}
          tone={pdfExport?.status === 'success' ? 'success' : 'warning'}
        />
        <ExportItem icon={Network} label="Webhook 摘要" meta="不发送密钥与完整 Header" tone="info" />
      </div>
      <button
        className="primary-button wide-button"
        disabled={isBusy}
        type="button"
        onClick={() => {
          void onExport()
        }}
      >
        <ShieldCheck size={16} aria-hidden="true" />
        执行导出检查
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
