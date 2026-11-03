const condensedCountFormat = new Intl.NumberFormat('en', { notation: 'compact', maximumFractionDigits: 0, roundingMode: 'floor' })
export const getCondensedCount = (count: number) => {
    const unit = count < 1000 ? 1 : 1000 ** Math.floor(Math.log10(count) / 3)
    return `${condensedCountFormat.format(count)}${count % unit === 0 ? '' : '+'}`
}
