{{- define "kooix-gate.name" -}}
{{- .Chart.Name | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "kooix-gate.fullname" -}}
{{- printf "%s-%s" .Release.Name (include "kooix-gate.name" .) | trunc 63 | trimSuffix "-" -}}
{{- end -}}
