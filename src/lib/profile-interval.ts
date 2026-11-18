/** Numeric inputs become undefined when cleared, and numbers when edited. */
export function validProfileInterval(value: unknown): boolean {
    const text = String(value ?? '').trim();
    const minutes = Number(text);
    return text !== '' && Number.isInteger(minutes) && minutes >= 0 && minutes <= 4294967295;
}
