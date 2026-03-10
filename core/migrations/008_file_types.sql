-- Migration 008: File types seed (200+ extensions with categories and hex colors)

CREATE TABLE IF NOT EXISTS file_types (
    extension   TEXT PRIMARY KEY NOT NULL,
    category    TEXT NOT NULL,              -- FileCategory name
    label       TEXT NOT NULL,              -- Human-readable label
    color       TEXT NOT NULL               -- Hex color for UI
);

-- Images
INSERT INTO file_types (extension, category, label, color) VALUES
('jpg', 'Image', 'JPEG Image', '#4CAF50'),
('jpeg', 'Image', 'JPEG Image', '#4CAF50'),
('png', 'Image', 'PNG Image', '#66BB6A'),
('gif', 'Image', 'GIF Image', '#81C784'),
('bmp', 'Image', 'Bitmap', '#A5D6A7'),
('svg', 'Image', 'SVG Vector', '#C8E6C9'),
('webp', 'Image', 'WebP Image', '#43A047'),
('ico', 'Image', 'Icon', '#388E3C'),
('tiff', 'Image', 'TIFF Image', '#2E7D32'),
('tif', 'Image', 'TIFF Image', '#2E7D32'),
('heic', 'Image', 'HEIC Photo', '#1B5E20'),
('heif', 'Image', 'HEIF Photo', '#1B5E20'),
('avif', 'Image', 'AVIF Image', '#4CAF50'),
('raw', 'Image', 'RAW Photo', '#33691E'),
('cr2', 'Image', 'Canon RAW', '#33691E'),
('nef', 'Image', 'Nikon RAW', '#33691E'),
('arw', 'Image', 'Sony RAW', '#33691E'),
('dng', 'Image', 'Digital Negative', '#33691E'),
('psd', 'Image', 'Photoshop', '#1A237E'),
('ai', 'Image', 'Illustrator', '#FF6F00'),
('eps', 'Image', 'Encapsulated PS', '#E65100'),
('jxl', 'Image', 'JPEG XL', '#4CAF50');

-- Video
INSERT INTO file_types (extension, category, label, color) VALUES
('mp4', 'Video', 'MP4 Video', '#F44336'),
('mkv', 'Video', 'Matroska', '#EF5350'),
('avi', 'Video', 'AVI Video', '#E53935'),
('mov', 'Video', 'QuickTime', '#D32F2F'),
('wmv', 'Video', 'Windows Media', '#C62828'),
('flv', 'Video', 'Flash Video', '#B71C1C'),
('webm', 'Video', 'WebM Video', '#FF5252'),
('m4v', 'Video', 'MPEG-4 Video', '#FF1744'),
('mpg', 'Video', 'MPEG Video', '#D50000'),
('mpeg', 'Video', 'MPEG Video', '#D50000'),
('3gp', 'Video', '3GPP Video', '#FF8A80'),
('m2ts', 'Video', 'MPEG-2 TS', '#E57373'),
('mts', 'Video', 'MPEG-2 TS', '#E57373'),
('vob', 'Video', 'DVD Video', '#EF9A9A');

-- Audio
INSERT INTO file_types (extension, category, label, color) VALUES
('mp3', 'Audio', 'MP3 Audio', '#9C27B0'),
('wav', 'Audio', 'WAV Audio', '#AB47BC'),
('flac', 'Audio', 'FLAC Lossless', '#BA68C8'),
('aac', 'Audio', 'AAC Audio', '#CE93D8'),
('ogg', 'Audio', 'OGG Audio', '#E1BEE7'),
('wma', 'Audio', 'Windows Media', '#8E24AA'),
('m4a', 'Audio', 'MPEG-4 Audio', '#7B1FA2'),
('aiff', 'Audio', 'AIFF Audio', '#6A1B9A'),
('opus', 'Audio', 'Opus Audio', '#4A148C'),
('mid', 'Audio', 'MIDI', '#EA80FC'),
('midi', 'Audio', 'MIDI', '#EA80FC');

-- Documents
INSERT INTO file_types (extension, category, label, color) VALUES
('pdf', 'Document', 'PDF Document', '#2196F3'),
('doc', 'Document', 'Word Document', '#1976D2'),
('docx', 'Document', 'Word Document', '#1976D2'),
('xls', 'Document', 'Excel Sheet', '#1B5E20'),
('xlsx', 'Document', 'Excel Sheet', '#1B5E20'),
('ppt', 'Document', 'PowerPoint', '#E65100'),
('pptx', 'Document', 'PowerPoint', '#E65100'),
('odt', 'Document', 'OpenDoc Text', '#42A5F5'),
('ods', 'Document', 'OpenDoc Sheet', '#66BB6A'),
('odp', 'Document', 'OpenDoc Slides', '#FFA726'),
('rtf', 'Document', 'Rich Text', '#90CAF9'),
('txt', 'Document', 'Plain Text', '#BBDEFB'),
('csv', 'Document', 'CSV Data', '#E3F2FD'),
('tsv', 'Document', 'TSV Data', '#E3F2FD'),
('md', 'Document', 'Markdown', '#64B5F6'),
('tex', 'Document', 'LaTeX', '#0D47A1'),
('epub', 'Document', 'EPUB Book', '#1565C0'),
('mobi', 'Document', 'Kindle Book', '#0D47A1'),
('pages', 'Document', 'Apple Pages', '#42A5F5'),
('numbers', 'Document', 'Apple Numbers', '#66BB6A'),
('keynote', 'Document', 'Apple Keynote', '#FFA726');

-- Code
INSERT INTO file_types (extension, category, label, color) VALUES
('rs', 'Code', 'Rust', '#FF6D00'),
('py', 'Code', 'Python', '#FFC107'),
('js', 'Code', 'JavaScript', '#FFEB3B'),
('ts', 'Code', 'TypeScript', '#1976D2'),
('tsx', 'Code', 'TypeScript React', '#1976D2'),
('jsx', 'Code', 'JavaScript React', '#FFEB3B'),
('html', 'Code', 'HTML', '#FF5722'),
('css', 'Code', 'CSS', '#2196F3'),
('scss', 'Code', 'SCSS', '#E91E63'),
('less', 'Code', 'LESS', '#2196F3'),
('json', 'Code', 'JSON', '#FFA000'),
('xml', 'Code', 'XML', '#FF6F00'),
('yaml', 'Code', 'YAML', '#FFA000'),
('yml', 'Code', 'YAML', '#FFA000'),
('toml', 'Code', 'TOML', '#795548'),
('sql', 'Code', 'SQL', '#00BCD4'),
('sh', 'Code', 'Shell Script', '#4CAF50'),
('bat', 'Code', 'Batch Script', '#607D8B'),
('ps1', 'Code', 'PowerShell', '#0288D1'),
('c', 'Code', 'C', '#1565C0'),
('cpp', 'Code', 'C++', '#0277BD'),
('h', 'Code', 'C Header', '#01579B'),
('hpp', 'Code', 'C++ Header', '#01579B'),
('java', 'Code', 'Java', '#F44336'),
('kt', 'Code', 'Kotlin', '#7C4DFF'),
('swift', 'Code', 'Swift', '#FF6D00'),
('go', 'Code', 'Go', '#00BCD4'),
('rb', 'Code', 'Ruby', '#F44336'),
('php', 'Code', 'PHP', '#7986CB'),
('cs', 'Code', 'C#', '#7B1FA2'),
('dart', 'Code', 'Dart', '#00BCD4'),
('lua', 'Code', 'Lua', '#3F51B5'),
('r', 'Code', 'R', '#1565C0'),
('scala', 'Code', 'Scala', '#D32F2F'),
('ex', 'Code', 'Elixir', '#7B1FA2'),
('exs', 'Code', 'Elixir Script', '#7B1FA2'),
('erl', 'Code', 'Erlang', '#D32F2F'),
('hs', 'Code', 'Haskell', '#7986CB'),
('ml', 'Code', 'OCaml', '#FF6F00'),
('vue', 'Code', 'Vue.js', '#4CAF50'),
('svelte', 'Code', 'Svelte', '#FF3E00'),
('zig', 'Code', 'Zig', '#F7A41D'),
('nim', 'Code', 'Nim', '#FFE953'),
('v', 'Code', 'V', '#536DFE'),
('wasm', 'Code', 'WebAssembly', '#654FF0'),
('graphql', 'Code', 'GraphQL', '#E10098'),
('proto', 'Code', 'Protocol Buffers', '#4CAF50');

-- Archives
INSERT INTO file_types (extension, category, label, color) VALUES
('zip', 'Archive', 'ZIP Archive', '#795548'),
('tar', 'Archive', 'TAR Archive', '#8D6E63'),
('gz', 'Archive', 'Gzip', '#A1887F'),
('bz2', 'Archive', 'Bzip2', '#BCAAA4'),
('xz', 'Archive', 'XZ', '#D7CCC8'),
('7z', 'Archive', '7-Zip', '#5D4037'),
('rar', 'Archive', 'RAR Archive', '#4E342E'),
('zst', 'Archive', 'Zstandard', '#3E2723'),
('lz4', 'Archive', 'LZ4', '#6D4C41'),
('iso', 'Archive', 'Disk Image', '#4E342E'),
('dmg', 'Archive', 'macOS Image', '#4E342E'),
('deb', 'Archive', 'Debian Package', '#E91E63'),
('rpm', 'Archive', 'RPM Package', '#F44336'),
('apk', 'Archive', 'Android Package', '#4CAF50'),
('msi', 'Archive', 'Windows Installer', '#2196F3');

-- Fonts
INSERT INTO file_types (extension, category, label, color) VALUES
('ttf', 'Font', 'TrueType Font', '#607D8B'),
('otf', 'Font', 'OpenType Font', '#78909C'),
('woff', 'Font', 'Web Font', '#90A4AE'),
('woff2', 'Font', 'Web Font 2', '#B0BEC5');

-- 3D / CAD
INSERT INTO file_types (extension, category, label, color) VALUES
('obj', 'Model3D', '3D Object', '#00BCD4'),
('fbx', 'Model3D', 'Autodesk FBX', '#0097A7'),
('stl', 'Model3D', 'Stereolithography', '#00838F'),
('gltf', 'Model3D', 'GL Transmission', '#006064'),
('glb', 'Model3D', 'GL Binary', '#006064'),
('blend', 'Model3D', 'Blender', '#FF6F00'),
('step', 'Model3D', 'STEP CAD', '#37474F'),
('iges', 'Model3D', 'IGES CAD', '#455A64');

-- Executables / Binary
INSERT INTO file_types (extension, category, label, color) VALUES
('exe', 'Executable', 'Windows Exe', '#F44336'),
('dll', 'Executable', 'Dynamic Library', '#EF5350'),
('so', 'Executable', 'Shared Object', '#E53935'),
('dylib', 'Executable', 'macOS Library', '#D32F2F'),
('app', 'Executable', 'macOS App', '#C62828'),
('bin', 'Executable', 'Binary', '#B71C1C'),
('elf', 'Executable', 'Linux Binary', '#FF1744');

-- Database
INSERT INTO file_types (extension, category, label, color) VALUES
('db', 'Database', 'Database', '#FF9800'),
('sqlite', 'Database', 'SQLite', '#FF9800'),
('sqlite3', 'Database', 'SQLite 3', '#FF9800'),
('mdb', 'Database', 'Access DB', '#F57C00');

-- Config
INSERT INTO file_types (extension, category, label, color) VALUES
('ini', 'Config', 'INI Config', '#9E9E9E'),
('conf', 'Config', 'Config File', '#9E9E9E'),
('cfg', 'Config', 'Config File', '#9E9E9E'),
('env', 'Config', 'Environment', '#9E9E9E'),
('properties', 'Config', 'Properties', '#9E9E9E'),
('lock', 'Config', 'Lock File', '#757575'),
('log', 'Config', 'Log File', '#616161'),
('gitignore', 'Config', 'Git Ignore', '#424242'),
('dockerignore', 'Config', 'Docker Ignore', '#424242'),
('editorconfig', 'Config', 'EditorConfig', '#9E9E9E');

-- Data / Scientific
INSERT INTO file_types (extension, category, label, color) VALUES
('parquet', 'Data', 'Apache Parquet', '#00BCD4'),
('arrow', 'Data', 'Apache Arrow', '#00ACC1'),
('hdf5', 'Data', 'HDF5 Data', '#0097A7'),
('h5', 'Data', 'HDF5 Data', '#0097A7'),
('npy', 'Data', 'NumPy Array', '#1976D2'),
('npz', 'Data', 'NumPy Archive', '#1976D2'),
('pkl', 'Data', 'Python Pickle', '#FFC107'),
('pt', 'Data', 'PyTorch Model', '#EE4C2C'),
('onnx', 'Data', 'ONNX Model', '#005CED'),
('safetensors', 'Data', 'SafeTensors', '#FF6F00');

-- Notebooks / Interactive
INSERT INTO file_types (extension, category, label, color) VALUES
('ipynb', 'Notebook', 'Jupyter Notebook', '#F37626'),
('rmd', 'Notebook', 'R Markdown', '#1565C0'),
('qmd', 'Notebook', 'Quarto', '#75AADB');

-- Markup / Templating
INSERT INTO file_types (extension, category, label, color) VALUES
('rst', 'Markup', 'reStructuredText', '#607D8B'),
('adoc', 'Markup', 'AsciiDoc', '#E91E63'),
('org', 'Markup', 'Org Mode', '#77AA99'),
('pug', 'Markup', 'Pug Template', '#A86454'),
('haml', 'Markup', 'HAML', '#ECE2A9'),
('ejs', 'Markup', 'EJS Template', '#B4CA65'),
('hbs', 'Markup', 'Handlebars', '#F0772B'),
('jinja', 'Markup', 'Jinja2', '#B41717'),
('j2', 'Markup', 'Jinja2', '#B41717');

-- Shaders / GPU
INSERT INTO file_types (extension, category, label, color) VALUES
('glsl', 'Shader', 'GLSL Shader', '#5C6BC0'),
('hlsl', 'Shader', 'HLSL Shader', '#42A5F5'),
('wgsl', 'Shader', 'WGSL Shader', '#26C6DA'),
('frag', 'Shader', 'Fragment Shader', '#7E57C2'),
('vert', 'Shader', 'Vertex Shader', '#66BB6A'),
('comp', 'Shader', 'Compute Shader', '#FFA726');

-- Certificates / Security
INSERT INTO file_types (extension, category, label, color) VALUES
('pem', 'Certificate', 'PEM Certificate', '#FF5722'),
('crt', 'Certificate', 'Certificate', '#FF7043'),
('key', 'Certificate', 'Private Key', '#E64A19'),
('csr', 'Certificate', 'Cert Request', '#BF360C'),
('p12', 'Certificate', 'PKCS12', '#DD2C00'),
('pfx', 'Certificate', 'PKCS12', '#DD2C00');

-- Misc
INSERT INTO file_types (extension, category, label, color) VALUES
('map', 'Misc', 'Source Map', '#9E9E9E'),
('LICENSE', 'Misc', 'License', '#455A64'),
('Makefile', 'Misc', 'Makefile', '#795548'),
('Dockerfile', 'Misc', 'Dockerfile', '#2196F3'),
('diff', 'Misc', 'Diff/Patch', '#FF9800'),
('patch', 'Misc', 'Patch File', '#FF9800');
