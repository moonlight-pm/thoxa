// Tutorial sections
export const sections = [
  {
    id: 'basics',
    title: '1. Basics',
    subsections: [
      { id: 'hello-world', title: 'Hello World' },
      { id: 'comments', title: 'Comments' },
      { id: 'variables', title: 'Variables' },
      { id: 'mutability', title: 'Mutability' },
    ],
  },
  {
    id: 'types',
    title: '2. Types',
    subsections: [
      { id: 'basic-types', title: 'Basic Types' },
      { id: 'arrays', title: 'Arrays' },
      { id: 'custom-types', title: 'Custom Types' },
    ],
  },
  {
    id: 'functions',
    title: '3. Functions',
    subsections: [
      { id: 'defining-functions', title: 'Defining Functions' },
      { id: 'parameters', title: 'Parameters & Returns' },
      { id: 'methods', title: 'Type Methods' },
    ],
  },
  {
    id: 'control-flow',
    title: '4. Control Flow',
    subsections: [
      { id: 'if-else', title: 'If/Else' },
      { id: 'while-loops', title: 'While Loops' },
      { id: 'for-loops', title: 'For Loops' },
    ],
  },
]

// Content for each section (code will be loaded from YAML)
export const content: Record<
  string,
  { title: string; description: string; explanation?: string }
> = {
  'hello-world': {
    title: 'Hello World',
    description: 'The traditional first program in any language.',
    explanation:
      'In Thoxa, code runs directly at the top level. The `println` function outputs text to the console. You can define functions if needed, but a `main` function is not required.',
  },
  comments: {
    title: 'Comments',
    description: 'Comments help document your code.',
    explanation:
      'Thoxa uses `#` for single-line comments. Multi-line comments are not yet supported.',
  },
  variables: {
    title: 'Variables',
    description: 'Variables store data for use in your program.',
    explanation:
      'Use `let` to declare immutable variables. Once set, their values cannot be changed.',
  },
  mutability: {
    title: 'Mutability',
    description: 'Sometimes you need variables that can change.',
    explanation:
      'Use `mut` to declare mutable variables. These can be reassigned after creation.',
  },
  'basic-types': {
    title: 'Basic Types',
    description: 'Thoxa has several built-in data types.',
    explanation:
      'Thoxa supports integers, floating-point numbers, strings, and booleans.',
  },
  arrays: {
    title: 'Arrays',
    description: 'Arrays store collections of values.',
    explanation:
      'Arrays are created with square brackets. Access elements using bracket notation with 0-based indices.',
  },
  'custom-types': {
    title: 'Custom Types',
    description: 'Define your own data structures.',
    explanation:
      'Types are defined with constructor parameters and exported fields. Use `export mut` for mutable fields and `export let` for immutable ones.',
  },
  'defining-functions': {
    title: 'Defining Functions',
    description: 'Functions encapsulate reusable code.',
    explanation:
      'Functions are defined with `fn`. They can take parameters and be called multiple times.',
  },
  parameters: {
    title: 'Parameters & Returns',
    description: 'Functions can accept input and return output.',
    explanation:
      'Functions can return values using `return` or by making the last expression the return value.',
  },
  methods: {
    title: 'Type Methods',
    description: 'Types can have associated methods.',
    explanation:
      "Types can have methods defined with `export fn`. These methods can access and modify the type's fields.",
  },
  'if-else': {
    title: 'If/Else',
    description: 'Make decisions in your code.',
    explanation:
      'Use `if` and `else` to execute different code based on conditions.',
  },
  'while-loops': {
    title: 'While Loops',
    description: 'Repeat code while a condition is true.',
    explanation:
      'While loops continue executing as long as their condition is true.',
  },
  'for-loops': {
    title: 'For Loops',
    description: 'Iterate over collections.',
    explanation: 'For loops iterate over arrays and other collections.',
  },
}
