require("strudel").setup()
local server = require("strudelserver").start_server()
server.open_site()

vim.keymap.set("n", "<leader>r", ":restart<CR>")
