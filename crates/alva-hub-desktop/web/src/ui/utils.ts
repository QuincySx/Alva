// INPUT:  Optional class-name values from UI component props
// OUTPUT: A stable space-delimited class-name string
// POS:    Tiny dependency-free utility shared by the ALVA design system

export function cx(...values: Array<string | false | null | undefined>): string {
  return values.filter(Boolean).join(" ");
}
