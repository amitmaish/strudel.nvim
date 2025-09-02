local M = {}

function M.setup()
	vim.api.nvim_create_user_command("StrudelStart", function()
		if M.strudelserver == nil then
			M.strudelserver = require("strudelserver").start_server()
			vim.api.nvim_create_autocmd("ExitPre", {
				callback = function()
					if M.strudelserver ~= nil then
						M.strudelserver.quit_server(M.strudelserver.server_handle)
					end
				end,
			})
		else
			print("strudel server already running")
		end
	end, {})

	vim.api.nvim_create_user_command("StrudelGetPort", function()
		if M.strudelserver ~= nil then
			print(M.strudelserver.get_port())
		else
			print("start the strudel server before getting port")
		end
	end, {})

	vim.api.nvim_create_user_command("StrudelOpen", function()
		if M.strudelserver ~= nil then
			M.strudelserver.open_site()
		else
			print("start the strudel server before opening the site")
		end
	end, {})

	vim.api.nvim_create_user_command("StrudelPlay", function()
		if M.strudelserver ~= nil then
			M.strudelserver.play()
		end
	end, {})

	vim.api.nvim_create_user_command("StrudelQuitServer", function()
		if M.strudelserver ~= nil then
			M.strudelserver.quit_server(M.strudelserver.server_handle)
			M.strudelserver = nil
		else
			print("strudel server not runing")
		end
	end, {})
end

local current_buffer_as_string = function()
	local content = vim.api.nvim_buf_get_lines(vim.api.nvim_get_current_buf(), 0, -1, false)
	return table.concat(content, "\n")
end

vim.api.nvim_create_user_command("StrudelUpdateCode", function()
	if M.strudelserver ~= nil then
		M.strudelserver.update_code(current_buffer_as_string())
	else
		print("strudel server not running")
	end
end, {})

return M
