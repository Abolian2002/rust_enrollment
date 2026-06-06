const PROVINCE_CODE_ALIASES: Record<string, string> = {
  hlj: "HLJ",
  heilongjiang: "HLJ",
  "黑龙江": "HLJ",
  jl: "JL",
  jilin: "JL",
  "吉林": "JL",
  ln: "LN",
  liaoning: "LN",
  "辽宁": "LN",
  heb: "HEB",
  hebei: "HEB",
  "河北": "HEB",
  sd: "SD",
  shandong: "SD",
  "山东": "SD",
  hen: "HEN",
  henan: "HEN",
  "河南": "HEN"
};

const PROVINCE_DISPLAY_NAMES: Record<string, string> = {
  HLJ: "黑龙江",
  JL: "吉林",
  LN: "辽宁",
  HEB: "河北",
  SD: "山东",
  HEN: "河南"
};

const MAJOR_SLUG_ALIASES: Record<string, string> = {
  education: "education",
  "教育学": "education",
  preschooleducation: "preschool-education",
  "学前教育": "preschool-education",
  chineselanguageandliterature: "chinese-language-and-literature",
  "汉语言文学": "chinese-language-and-literature",
  "中文": "chinese-language-and-literature",
  english: "english",
  "英语": "english",
  mathematicsandappliedmathematics: "mathematics-and-applied-mathematics",
  "数学与应用数学": "mathematics-and-applied-mathematics",
  "数学": "mathematics-and-applied-mathematics",
  physics: "physics",
  "物理学": "physics",
  "物理": "physics",
  computerscienceandtechnology: "computer-science-and-technology",
  "计算机科学与技术": "computer-science-and-technology",
  "计算机": "computer-science-and-technology",
  publicadministration: "public-administration",
  "公共管理": "public-administration",
  "公共事业管理": "public-administration"
};

const MAJOR_SEARCH_ALIASES: Record<string, string> = {
  "教育学": "Education",
  "学前教育": "Preschool Education",
  "汉语言文学": "Chinese Language and Literature",
  "中文": "Chinese Language and Literature",
  "英语": "English",
  "数学与应用数学": "Mathematics and Applied Mathematics",
  "数学": "Mathematics",
  "物理学": "Physics",
  "物理": "Physics",
  "计算机科学与技术": "Computer Science and Technology",
  "计算机": "Computer Science and Technology",
  "公共管理": "Public Administration",
  "公共事业管理": "Public Administration"
};

function normalizeKey(value: string) {
  return value.trim().toLowerCase().replace(/[\s_-]+/g, "");
}

export function normalizeProvinceCode(input: string, fallback: string) {
  return PROVINCE_CODE_ALIASES[normalizeKey(input)] ?? fallback;
}

export function getProvinceDisplayName(input: string, fallback?: string) {
  return PROVINCE_DISPLAY_NAMES[input] ?? fallback ?? input;
}

export function normalizeMajorSlug(input: string, fallback: string) {
  const key = normalizeKey(input);
  return MAJOR_SLUG_ALIASES[key] ?? (input.trim() || fallback);
}

export function normalizeMajorSearch(input: string, fallback: string) {
  const trimmed = input.trim();
  if (!trimmed) {
    return fallback;
  }

  return MAJOR_SEARCH_ALIASES[trimmed] ?? trimmed;
}
