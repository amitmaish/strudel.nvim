local M = {}

function M.setup()
	vim.api.nvim_create_user_command("StrudelStart", function()
		M.strudelserver = require("strudelserver").start_server()
		vim.api.nvim_create_autocmd("ExitPre", {
			callback = function()
				if M.strudelserver ~= nil then
					M.strudelserver.quit_server(M.strudelserver.server_handle)
				end
			end,
		})
	end, {})

	vim.api.nvim_create_user_command("StrudelGetPort", function()
		if M.strudelserver ~= nil then
			print(M.strudelserver.get_port())
		end
	end, {})

	vim.api.nvim_create_user_command("StrudelQuitServer", function()
		if M.strudelserver ~= nil then
			M.strudelserver.quit_server(M.strudelserver.server_handle)
		end
	end, {})
end

return M
