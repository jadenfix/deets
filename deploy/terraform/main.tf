# ============================================================================
# AETHER INFRASTRUCTURE - Terraform Configuration
# ============================================================================
# PURPOSE: Provision cloud infrastructure for Aether testnet/mainnet
#
# RESOURCES:
#   - VPC with multi-AZ subnets
#   - EKS cluster for validators
#   - NVMe-backed node pools
#   - S3 buckets (snapshots, artifacts)
#   - CloudWatch/monitoring
#   - NLB for RPC endpoints
#
# USAGE:
#   terraform init
#   terraform plan
#   terraform apply
# ============================================================================

terraform {
  required_version = ">= 1.0"
  
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
  }
  
  backend "s3" {
    bucket = "aether-terraform-state"
    key    = "infrastructure/terraform.tfstate"
    region = "us-east-1"
  }
}

provider "aws" {
  region = var.aws_region
}

# VPC
module "vpc" {
  source  = "terraform-aws-modules/vpc/aws"
  version = "~> 5.0"
  
  name = "aether-${var.environment}"
  cidr = "10.0.0.0/16"
  
  azs             = ["us-east-1a", "us-east-1b", "us-east-1c"]
  private_subnets = ["10.0.1.0/24", "10.0.2.0/24", "10.0.3.0/24"]
  public_subnets  = ["10.0.101.0/24", "10.0.102.0/24", "10.0.103.0/24"]
  
  enable_nat_gateway = true
  enable_vpn_gateway = false
  
  tags = {
    Environment = var.environment
    Project     = "aether"
  }
}

# EKS Cluster
module "eks" {
  source  = "terraform-aws-modules/eks/aws"
  version = "~> 19.0"
  
  cluster_name    = "aether-${var.environment}"
  cluster_version = "1.28"
  
  vpc_id     = module.vpc.vpc_id
  subnet_ids = module.vpc.private_subnets
  
  # Validator node group (i3en.6xlarge with NVMe)
  eks_managed_node_groups = {
    validators = {
      desired_size = 4
      min_size     = 4
      max_size     = 10
      
      instance_types = ["i3en.6xlarge"]
      capacity_type  = "ON_DEMAND"
      
      labels = {
        role = "validator"
      }
      
      taints = {
        dedicated = {
          key    = "validator"
          value  = "true"
          effect = "NoSchedule"
        }
      }
    }
    
    # RPC node group
    rpc = {
      desired_size = 3
      min_size     = 2
      max_size     = 20
      
      instance_types = ["r6i.4xlarge"]
      capacity_type  = "SPOT"
      
      labels = {
        role = "rpc"
      }
    }
  }
}

# S3 Bucket for snapshots
resource "aws_s3_bucket" "snapshots" {
  bucket = "aether-${var.environment}-snapshots"
  
  tags = {
    Environment = var.environment
    Purpose     = "state-snapshots"
  }
}

resource "aws_s3_bucket_versioning" "snapshots" {
  bucket = aws_s3_bucket.snapshots.id
  
  versioning_configuration {
    status = "Enabled"
  }
}

# Variables
variable "aws_region" {
  default = "us-east-1"
}

variable "environment" {
  description = "Environment name (devnet, testnet, mainnet)"
  type        = string
}

# Outputs
output "eks_cluster_endpoint" {
  value = module.eks.cluster_endpoint
}

output "snapshots_bucket" {
  value = aws_s3_bucket.snapshots.id
}

