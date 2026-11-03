'use client'

import { useId, useState, type ReactNode } from 'react'
import { ArrowUpRight } from 'lucide-react'
import { toast } from 'sonner'
import { Button } from '@/shadcn/components/ui/button'
import { Checkbox } from '@/shadcn/components/ui/checkbox'
import { Input } from '@/shadcn/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/shadcn/components/ui/select'
import { Textarea } from '@/shadcn/components/ui/textarea'
import { getFieldLabel, getInitialFieldValue, isReferenceTarget, type EditorFieldSchema, type EditorValue, type StoredWorld } from '../editorObjectSchema'
import { degreesToQuaternion, quaternionToDegrees } from '../editorRotation'
import { areEditorValuesEqual, type EditorPropertyUpdate } from '../editorProperties'

// Property controls edit React values only. Temporary numeric text stays here until it is valid.
const inputClass = 'h-7 min-w-0 w-full max-w-24 rounded border-0 bg-gray-50 px-2 py-0 text-[11px] md:text-[11px] text-gray-800 shadow-none outline-none transition-colors hover:bg-gray-100 focus:bg-white focus-visible:ring-1 focus-visible:ring-blue-500 disabled:opacity-50'
// TODO: AI-PICKED-VALUE: Cap numeric fields at 6rem and keep grouped components together at the same maximum width.
const numberGroupClass = 'grid grid-cols-[repeat(3,minmax(0,6rem))] gap-1'
// Every property uses the same two columns, including checkboxes and controls in narrow panels.
// TODO: AI-PICKED-VALUE: Give field names 7rem, matching the Type and ID rows in the inspector.
// Rows and headings share a 28px minimum height; every vertical stack uses the same 6px gap.
export const PropertyRow = ({ label, enableControl, children }: { label: ReactNode, enableControl: ReactNode, children: ReactNode }) => <div className="grid min-h-7 min-w-0 grid-cols-[7rem_minmax(0,1fr)] items-start gap-2">
    <div className="pt-1.5 text-[11px] leading-4 text-gray-500">{label}</div><div className="flex min-w-0 items-start gap-2">{enableControl}<div className="min-w-0 flex-1">{children}</div></div>
</div>
// Shadcn provides menu focus and keyboard navigation while the trigger stays quiet in the property table.
const PropertySelect = ({ id, value, onValueChange, children }: { id: string, value: string, onValueChange: (value: string) => void, children: ReactNode }) => <Select autoComplete="off" value={value} onValueChange={onValueChange}>
    <SelectTrigger id={id} data-lpignore="true" className="data-[size=default]:h-7 min-w-0 w-full max-w-24 cursor-pointer gap-1 rounded border-0 bg-transparent px-0 py-0 text-[11px] text-gray-800 focus-visible:ring-1 focus-visible:ring-blue-500 [&>svg]:size-3"><SelectValue placeholder="Mixed" /></SelectTrigger>
    <SelectContent align="start">{children}</SelectContent>
</Select>
const NumberField = ({ label, axis, value, schema, onChange }: { label: string, axis: string | null, value: number | null, schema: EditorFieldSchema, onChange: (value: number) => void }) => {
    const [draft, setDraft] = useState<string | null>(null)
    const [isInvalid, setIsInvalid] = useState(false)
    const isInteger = schema.type === 'integer'
    const factor = schema.control === 'negativeDegrees' ? -180 / Math.PI : schema.control === 'degrees' ? 180 / Math.PI : 1
    const displayed = value === null ? null : value * factor
    // Rounding is only for display. Focusing reveals the full value, and an unchanged blur never writes it back.
    // TODO: AI-PICKED-VALUE: Show up to three decimal places, retaining significant digits for very small nonzero values.
    const formatted = displayed === null ? '' : String(Number(Math.abs(displayed) > 0 && Math.abs(displayed) < 0.001 ? displayed.toPrecision(3) : displayed.toFixed(3)))
    // Apply valid numbers while typing, retaining the entered text until blur and reporting unfinished input only then.
    const parseNumber = (text: string) => {
        if ((displayed === null && !text.trim()) || text === String(displayed)) return value
        const parsed = Number(text)
        let next = parsed / factor
        // Let users finish typing before applying the field's range in its stored units.
        if (schema.minimum !== undefined) next = Math.max(schema.minimum, next)
        if (schema.maximum !== undefined) next = Math.min(schema.maximum, next)
        if (isInteger) next = Math.round(next)
        if (!text.trim() || !Number.isFinite(parsed)) return 'Enter a finite number.'
        if (isInteger && !Number.isSafeInteger(next)) return 'Enter a whole number.'
        return next
    }
    // Step in the displayed units, including degrees. Integer fields must remain whole numbers.
    const step = schema.type === 'integer' ? 1 : 0.05
    const minimum = factor < 0 ? schema.maximum : schema.minimum
    const maximum = factor < 0 ? schema.minimum : schema.maximum
    const displayedMinimum = minimum === undefined ? undefined : minimum * factor
    const displayedMaximum = maximum === undefined ? undefined : maximum * factor
    // Huge storage limits lose small increments in native steppers. Commit still clamps to the full schema range.
    const stepMinimum = displayedMinimum === undefined || displayedMinimum + step === displayedMinimum ? undefined : displayedMinimum
    return <div className="min-w-0 w-full max-w-24 flex-1">
        <div className="relative">
            {axis && <span aria-hidden="true" className={`pointer-events-none absolute left-1.5 top-1/2 -translate-y-1/2 text-[10px] font-medium ${axis === 'X' || axis === 'R' ? 'text-rose-400' : axis === 'Y' || axis === 'G' ? 'text-emerald-500' : 'text-sky-500'}`}>{axis}</span>}
            <Input type={isInteger ? 'text' : 'number'} step={isInteger ? undefined : step} min={isInteger ? undefined : stepMinimum} max={isInteger ? undefined : displayedMaximum} title="" autoComplete="off" data-lpignore="true" aria-label={label} aria-invalid={isInvalid} inputMode={isInteger ? 'numeric' : 'decimal'} className={`${inputClass} tabular-nums ${axis ? 'pl-5' : ''}`} placeholder={value === null ? 'Mixed' : undefined} value={draft ?? formatted} onFocus={event => {
                // Native stepping uses the value attribute as its base; match the full precision shown on focus.
                if (draft === null) { event.currentTarget.defaultValue = displayed === null ? '' : String(displayed); setDraft(displayed === null ? '' : String(displayed)) }
            }} onChange={event => {
                setDraft(event.target.value); setIsInvalid(false)
                const next = parseNumber(event.target.value)
                if (typeof next === 'number' && next !== value) onChange(next)
            }} onBlur={() => {
                if (draft === null) return
                const next = parseNumber(draft)
                if (typeof next === 'string') { setIsInvalid(true); toast.error(`${label}: ${next}`, { id: 'editor-property-error' }); return }
                if (typeof next === 'number' && next !== value) onChange(next)
                setDraft(null); setIsInvalid(false)
            }} onKeyDown={event => {
                if (event.key === 'Enter' && !event.nativeEvent.isComposing) { event.preventDefault(); event.currentTarget.blur() }
                if (event.key === 'Escape') { event.preventDefault(); setDraft(null); setIsInvalid(false) }
            }} />
        </div>
    </div>
}

// Text stays as typed while it is being edited, and every change still applies at once. The editor keeps some
// text differently, like a name without spaces, and the field shows what it kept once focus leaves.
const TextField = ({ id, label, value, isMultiline, onChange }: { id: string | undefined, label: string | undefined, value: string | null, isMultiline: boolean, onChange: (value: string) => void }) => {
    const [draft, setDraft] = useState<string | null>(null)
    const props = { id, 'aria-label': label, autoComplete: 'off', 'data-lpignore': 'true', placeholder: value === null ? 'Mixed' : undefined, value: draft ?? value ?? '', onBlur: () => setDraft(null) }
    const change = (text: string) => { setDraft(text); onChange(text) }
    return isMultiline
        ? <Textarea {...props} rows={3} className={`${inputClass} h-auto max-w-80 resize-y py-1.5`} onChange={event => change(event.target.value)} />
        : <Input {...props} className={inputClass} onChange={event => change(event.target.value)} onKeyDown={event => { if (event.key === 'Enter' && !event.nativeEvent.isComposing) { event.preventDefault(); event.currentTarget.blur() } }} />
}

// Keep the chosen Euler angles while editing so equivalent quaternion representations do not move the other inputs.
const RotationField = ({ label, values, onChange }: { label: string, values: number[][], onChange: (update: EditorPropertyUpdate) => void }) => {
    const [rotation, setRotation] = useState({ quaternions: values, degrees: values.map(quaternionToDegrees) })
    const rotations = values.map((quaternion, index) => areEditorValuesEqual(quaternion, rotation.quaternions[index]) ? rotation.degrees[index] : quaternionToDegrees(quaternion))
    return <div className={numberGroupClass}>{rotations[0].map((angle, index) => <NumberField key={index} label={`${label} ${['X', 'Y', 'Z'][index]} (degrees)`} axis={['X', 'Y', 'Z'][index]} value={rotations.every(other => other[index] === angle) ? angle : null} schema={{ type: 'number' }} onChange={next => {
        const degrees = rotations.map(rotation => rotation.map((angle, axis) => axis === index ? next : angle))
        const quaternions = degrees.map(degreesToQuaternion)
        setRotation({ quaternions, degrees }); onChange(current => {
            const index = values.findIndex(value => areEditorValuesEqual(value, current))
            return index === -1 ? current : quaternions[index]
        })
    }} />)}</div>
}

export type PropertyFieldProps = { label: string, schema: EditorFieldSchema, values: EditorValue[], path: string[], world: StoredWorld, onEdit: (path: string[], update: EditorPropertyUpdate) => void, onSelect: (objectId: number) => void }

// Nullable fields and enum variants retain their Rust JSON shapes. Nested fields share one renderer.
export const EditorPropertyField = ({ label, schema: fieldSchema, values, path, world, onEdit, onSelect }: PropertyFieldProps) => {
    const id = useId()
    const value = values[0]
    const isMixed = values.some(other => !areEditorValuesEqual(value, other))
    const change = (next: EditorPropertyUpdate) => onEdit(path, next)
    let schema = fieldSchema
    let enableControl: ReactNode = null
    // Resolve optional values here so the checkbox and value share one label and one row.
    const inner = schema.anyOf?.find(option => option.type !== 'null')
    if (inner && schema.anyOf?.some(option => option.type === 'null')) {
        const initial = getInitialFieldValue(inner, world)
        enableControl = <Checkbox id={`${id}-enabled`} aria-label={`Enable ${label}`} className="my-2 size-3 shrink-0 [&_svg]:size-3" checked={values.every(value => value !== null) ? true : values.every(value => value === null) ? false : 'indeterminate'} disabled={initial === undefined && values.some(value => value === null)} onCheckedChange={checked => {
            if (!checked) change(null)
            else change((current, originalSchema) => {
                const inner = originalSchema.anyOf?.find(option => option.type !== 'null')
                return current ?? (inner ? getInitialFieldValue(inner, world) ?? null : null)
            })
        }} />
        if (values.some(value => value === null)) return <PropertyRow label={<label htmlFor={`${id}-enabled`}>{label}</label>} enableControl={enableControl}>{isMixed ? <span className="text-[11px] text-gray-500">Mixed</span> : null}</PropertyRow>
        schema = inner
    }
    if (schema.anyOf) {
        const options = schema.anyOf
        const active = options.findIndex(option => option.const === value || (value !== null && typeof value === 'object' && !Array.isArray(value) && Object.keys(option.properties ?? {}).some(key => Object.hasOwn(value, key))))
        const isVariantMixed = values.some(other => options.findIndex(option => option.const === other || (other !== null && typeof other === 'object' && !Array.isArray(other) && Object.keys(option.properties ?? {}).some(key => Object.hasOwn(other, key)))) !== active)
        const selected = isVariantMixed ? undefined : options[active]
        return <div className="grid min-w-0 gap-1.5">
            <PropertyRow label={<label htmlFor={id}>{label}</label>} enableControl={enableControl}>
            <PropertySelect id={id} value={isVariantMixed ? '' : String(active)} onValueChange={value => { const next = getInitialFieldValue(options[Number(value)], world); if (next !== undefined) change(next) }}>
                {options.map((option, index) => <SelectItem className="text-xs" key={index} value={String(index)}>{String(option.const ?? Object.keys(option.properties ?? {})[0])}</SelectItem>)}
            </PropertySelect>
            </PropertyRow>
            {selected?.type === 'object' && <EditorPropertyField label={label} schema={selected} values={values} path={path} world={world} onEdit={onEdit} onSelect={onSelect} />}
        </div>
    }
    if (schema.control === 'reference' && typeof value === 'number') {
        const targets = Object.values(world.objectOfId).filter(object => isReferenceTarget(schema, world, object)).toSorted((a, b) => a.name.localeCompare(b.name) || a.id - b.id)
        return <PropertyRow label={<label htmlFor={id}>{label}</label>} enableControl={enableControl}><div className="flex gap-1">
            <PropertySelect id={id} value={isMixed ? '' : String(value)} onValueChange={value => change(Number(value))}>
                {!targets.some(object => object.id === value) && <SelectItem className="text-xs" value={String(value)}>#{value} (unavailable)</SelectItem>}
                {targets.map(object => <SelectItem className="text-xs" key={object.id} value={String(object.id)}>{object.name} · #{object.id}</SelectItem>)}
            </PropertySelect>
            <Button type="button" variant="ghost" size="icon" aria-label={`Select ${label} object`} disabled={isMixed || !world.objectOfId[value]} className="size-7 rounded p-1 text-gray-500 hover:bg-gray-100 disabled:opacity-30" onClick={() => onSelect(value)}><ArrowUpRight className="size-4" /></Button>
        </div></PropertyRow>
    }
    if (schema.enum) return <PropertyRow label={<label htmlFor={id}>{label}</label>} enableControl={enableControl}><PropertySelect id={id} value={isMixed ? '' : String(value)} onValueChange={change}>{schema.enum.map(option => <SelectItem className="text-xs" key={String(option)} value={String(option)}>{String(option)}</SelectItem>)}</PropertySelect></PropertyRow>
    if (typeof value === 'boolean') return <PropertyRow label={<label htmlFor={id}>{label}</label>} enableControl={enableControl}><Checkbox id={id} className="my-2 size-3 [&_svg]:size-3" checked={isMixed ? 'indeterminate' : value} onCheckedChange={checked => change(checked === true)} /></PropertyRow>
    if (typeof value === 'number') return <PropertyRow enableControl={enableControl} label={<span>{label}{schema.control === 'degrees' || schema.control === 'negativeDegrees' ? ' (°)' : ''}</span>}><NumberField label={label} axis={null} value={isMixed ? null : value} schema={schema} onChange={change} /></PropertyRow>
    // TODO: AI-PICKED-VALUE: Allow text areas up to 20rem so multiline content has room beside its label.
    // Object properties are game data, so ask browsers and LastPass to leave them out of autofill.
    if (typeof value === 'string') return <PropertyRow label={<label htmlFor={id}>{label}</label>} enableControl={enableControl}><TextField id={id} label={undefined} value={isMixed ? null : value} isMultiline={path.at(-1) === 'text'} onChange={change} /></PropertyRow>
    if (Array.isArray(value)) {
        const isVector = schema.control === 'vector' || schema.control === 'angularVelocity'
        const displayLabel = schema.control === 'rotation' ? 'Rotation (°)' : schema.control === 'angularVelocity' ? `${label} (°/s)` : label
        // Contain the absolutely positioned legend so offscreen fields cannot increase the page's scroll height.
        return <fieldset className="relative min-w-0"><legend className="sr-only">{displayLabel}</legend><PropertyRow label={<span aria-hidden="true">{displayLabel}</span>} enableControl={enableControl}>
            {schema.control === 'rotation' && values.every(value => Array.isArray(value) && value.every(component => typeof component === 'number')) ? <RotationField label={label} values={values} onChange={change} /> : <div className={isVector ? numberGroupClass : 'grid gap-1.5'}>
                {!schema.prefixItems && isMixed && <span className="text-[11px] text-gray-500">Mixed</span>}
                {value.slice(0, Math.min(...values.map(value => Array.isArray(value) ? value.length : 0))).map((component, index) => {
                    const itemSchema = schema.prefixItems?.[index] ?? schema.items
                    if (!itemSchema || typeof itemSchema === 'boolean' || Array.isArray(itemSchema)) return null
                    return <div key={index} className="flex min-w-0 items-center gap-1">{typeof component === 'number' ? <NumberField label={`${label} ${['X', 'Y', 'Z'][index]}`} axis={isVector ? ['X', 'Y', 'Z'][index] : null} value={values.every(other => Array.isArray(other) && other[index] === component) ? component : null} schema={schema.control === 'angularVelocity' ? { ...itemSchema, control: 'degrees' } : itemSchema} onChange={next => change(current => Array.isArray(current) ? current.map((previous, axis) => axis === index ? next : previous) : current)} /> : <TextField id={undefined} label={`${label} ${index + 1}`} value={values.every(other => Array.isArray(other) && other[index] === component) ? String(component) : null} isMultiline={false} onChange={text => change(current => Array.isArray(current) ? current.map((previous, item) => item === index ? text : previous) : current)} />}
                        {!schema.prefixItems && <Button type="button" variant="ghost" size="icon" aria-label={`Remove ${label} ${index + 1}`} className="size-7 px-1 text-gray-500" onClick={() => change(current => Array.isArray(current) ? current.filter((_item, item) => item !== index) : current)}>×</Button>}
                    </div>
                })}
                {!schema.prefixItems && <Button type="button" variant="ghost" className="h-7 px-1 text-xs text-gray-500 hover:text-gray-950" onClick={() => change(current => Array.isArray(current) ? [...current, ''] : current)}>Add Entry</Button>}
            </div>}
        </PropertyRow></fieldset>
    }
    if (value !== null && typeof value === 'object' && schema.control === 'color' && typeof value.r === 'number' && typeof value.g === 'number' && typeof value.b === 'number') return <PropertyRow label={<label htmlFor={id}>{label}</label>} enableControl={enableControl}>
        <div className="flex min-w-0 items-center gap-1">{isMixed && <span className="text-[11px] text-gray-500">Mixed</span>}<Input id={id} type="color" aria-label={`${label} picker`} autoComplete="off" data-lpignore="true" className="h-7 min-w-0 w-full max-w-24 cursor-pointer rounded border-0 p-0 shadow-none" value={`#${[value.r, value.g, value.b].map(channel => Math.round(channel * 255).toString(16).padStart(2, '0')).join('')}`} onChange={event => { const hex = event.target.value; change({ r: parseInt(hex.slice(1, 3), 16) / 255, g: parseInt(hex.slice(3, 5), 16) / 255, b: parseInt(hex.slice(5, 7), 16) / 255 }) }} /></div>
    </PropertyRow>
    // Groups add no outer padding or margins, so nesting never increases the space between rows.
    if (value !== null && typeof value === 'object' && schema.properties) return <div role="group" aria-label={label} className="grid min-w-0 gap-1.5">
        {enableControl ? <PropertyRow label={<label htmlFor={`${id}-enabled`}>{label}</label>} enableControl={enableControl}>{null}</PropertyRow> : <div className="flex min-h-7 items-center text-[11px] font-semibold text-gray-700">{label}</div>}
        {Object.entries(schema.properties).map(([field, childSchema]) => typeof childSchema === 'boolean' || !Object.hasOwn(value, field) ? null : <EditorPropertyField key={field} label={getFieldLabel(field)} schema={childSchema} values={values.flatMap(value => value !== null && typeof value === 'object' && !Array.isArray(value) ? [value[field]] : [])} path={[...path, field]} world={world} onEdit={onEdit} onSelect={onSelect} />)}
    </div>
    return null
}
