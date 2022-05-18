# frozen_string_literal: true

# Copyright (c) Aptos
# SPDX-License-Identifier: Apache-2.0

class WelcomeController < ApplicationController
  layout 'it1'

  before_action :ensure_confirmed!, only: %i[it1]

  def index
    redirect_to it1_path if current_user && current_user.authorizations.where(provider: :discord).exists?
  end

  def it1
    redirect_to root_path unless user_signed_in?
  end
end
