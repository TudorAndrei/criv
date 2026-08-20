defmodule Acceptance.User do
  defstruct [:id, name: "unknown"]
end

defmodule Acceptance.Failure do
  defexception [:message]
end

defmodule Acceptance.Container do
  defmodule Child do
    def value(), do: :child
  end
end
